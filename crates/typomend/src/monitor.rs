// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resource usage of Typomend, sampled once a second for the charts on the
//! Usage page. The masked language model runs on the CPU inside this process,
//! so the numbers include it.
//!
//! CPU time and memory come from the process APIs. GPU use comes from the
//! same performance counters Task Manager reads, `GPU Engine` and `GPU
//! Process Memory`, whose instances are named after the process id. A
//! machine without them, such as a virtual machine without a GPU driver,
//! reports no GPU numbers.
//!
//! Sampling costs a few milliseconds of CPU each second, so it runs only
//! while the user has it switched on and the Usage page is open.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::w;

use crate::app;

/// Time between samples.
const INTERVAL: Duration = Duration::from_secs(1);

/// Samples kept for the charts, two minutes' worth.
pub const HISTORY: usize = 120;

/// Lets the engine counter report more than 100 when several engines of one
/// type are summed. Defined in pdh.h.
const PDH_FMT_NOCAP100: u32 = 0x8000;

/// Counter statuses that mark a usable value. Defined in pdhmsg.h.
const PDH_CSTATUS_VALID_DATA: u32 = 0;
const PDH_CSTATUS_NEW_DATA: u32 = 1;

/// Resource use of this process at one sample.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    /// Share of all logical processors, in percent.
    pub cpu: f32,
    /// Private bytes, the memory committed for this process alone.
    pub memory: u64,
    /// Utilization of the busiest GPU engine type, in percent, or `None`
    /// without GPU counters.
    pub gpu: Option<f32>,
    /// Dedicated GPU memory in bytes, or `None` without GPU counters.
    pub gpu_memory: Option<u64>,
}

/// The sampling thread and the samples it has taken.
#[derive(Debug, Default)]
pub struct Monitor {
    /// The last [`HISTORY`] samples, oldest first.
    samples: Mutex<VecDeque<Usage>>,
    /// Set to stop the running thread; `None` while none runs.
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl Monitor {
    /// Creates a monitor that is not sampling yet.
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// Starts sampling on a background thread, from an empty history.
    ///
    /// Posts [`app::WM_APP_MONITOR`] after every sample. Does nothing if
    /// sampling already runs.
    pub fn start(self: &Arc<Self>) {
        let mut stop = self.stop.lock();
        if stop.is_some() {
            return;
        }
        let flag = Arc::new(AtomicBool::new(false));
        *stop = Some(Arc::clone(&flag));
        drop(stop);
        self.samples.lock().clear();

        let monitor = Arc::clone(self);
        std::thread::spawn(move || {
            let mut sampler = Sampler::new();
            // CPU and GPU utilization are rates, so the first reading only
            // sets the baseline.
            sampler.sample();
            loop {
                std::thread::sleep(INTERVAL);
                let sample = sampler.sample();
                if flag.load(Ordering::Acquire) {
                    break;
                }
                let mut samples = monitor.samples.lock();
                samples.push_back(sample);
                while samples.len() > HISTORY {
                    samples.pop_front();
                }
                drop(samples);
                app::post(app::WM_APP_MONITOR);
            }
        });
    }

    /// Stops sampling and forgets the samples taken.
    pub fn stop(&self) {
        if let Some(flag) = self.stop.lock().take() {
            flag.store(true, Ordering::Release);
        }
        self.samples.lock().clear();
    }

    /// Returns the samples taken, oldest first.
    pub fn samples(&self) -> Vec<Usage> {
        self.samples.lock().iter().copied().collect()
    }
}

/// Returns the private bytes of this process, which grow by the memory a
/// model takes when it loads.
pub fn private_bytes() -> Option<u64> {
    // SAFETY: GetCurrentProcessId takes no arguments and cannot fail.
    let process = Process::open(unsafe { GetCurrentProcessId() })?;
    process.private_bytes()
}

/// An open process handle with query rights only, closed on drop.
struct Process(HANDLE);

impl Process {
    /// Opens process `pid`, or returns `None` when it has exited or denies
    /// access.
    fn open(pid: u32) -> Option<Self> {
        // SAFETY: OpenProcess takes no pointers. A handle it returns is
        // owned by the new value alone, which closes it on drop.
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .ok()
            .map(Self)
    }

    /// Returns the CPU time used so far, user and kernel, in 100 ns units.
    fn cpu_time(&self) -> Option<u64> {
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: `self.0` is an open handle with query rights, and each
        // out-pointer refers to a distinct, writable FILETIME.
        unsafe { GetProcessTimes(self.0, created, exited, kernel, user) }.ok()?;
        let ticks = |t: &FILETIME| u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime);
        Some(ticks(kernel) + ticks(user))
    }

    /// Returns the private bytes of the process, or `None` when the query
    /// fails.
    fn private_bytes(&self) -> Option<u64> {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        // SAFETY: `self.0` is an open handle with query rights. The EX
        // structure starts with PROCESS_MEMORY_COUNTERS and its size is
        // passed along, which is how the API tells the two apart.
        unsafe {
            GetProcessMemoryInfo(
                self.0,
                &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                counters.cb,
            )
        }
        .ok()?;
        Some(counters.PrivateUsage as u64)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from OpenProcess, is owned by this value
        // alone and is closed only here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Takes samples, remembering what the rates are computed from.
struct Sampler {
    /// Id of this process, which names its GPU counter instances.
    own_pid: u32,
    /// Logical processors, which the CPU share is spread over.
    processors: f64,
    /// When the last sample was taken.
    last: Instant,
    /// CPU time of this process at the last sample, in 100 ns units.
    cpu_time: Option<u64>,
    /// `None` on a machine without GPU counters.
    gpu: Option<GpuCounters>,
}

impl Sampler {
    fn new() -> Self {
        let mut info = SYSTEM_INFO::default();
        // SAFETY: `info` is a writable SYSTEM_INFO, the only argument.
        unsafe { GetSystemInfo(&mut info) };
        Self {
            // SAFETY: GetCurrentProcessId takes no arguments and cannot fail.
            own_pid: unsafe { GetCurrentProcessId() },
            processors: f64::from(info.dwNumberOfProcessors.max(1)),
            last: Instant::now(),
            cpu_time: None,
            gpu: GpuCounters::open(),
        }
    }

    /// Takes one sample. CPU and GPU use cover the time since the previous
    /// call, so the first call only sets the baseline and reports zero for
    /// both.
    fn sample(&mut self) -> Usage {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;

        let mut usage = Usage::default();
        if let Some(process) = Process::open(self.own_pid) {
            if let Some(time) = process.cpu_time() {
                if let Some(before) = self.cpu_time
                    && elapsed > 0.0
                {
                    // 100 ns ticks to seconds, over the time of all
                    // processors together.
                    let busy = time.saturating_sub(before) as f64 / 1e7;
                    usage.cpu = (busy / (elapsed * self.processors) * 100.0) as f32;
                }
                self.cpu_time = Some(time);
            }
            usage.memory = process.private_bytes().unwrap_or(0);
        }
        if let Some(gpu) = self.gpu.as_ref().map(GpuCounters::collect) {
            usage.gpu = Some(gpu.engine.get(&self.own_pid).copied().unwrap_or(0.0));
            usage.gpu_memory = Some(gpu.memory.get(&self.own_pid).copied().unwrap_or(0));
        }
        usage
    }
}

/// GPU use per process id from one collection of the GPU counters.
#[derive(Debug, Default)]
struct GpuReading {
    /// Utilization in percent.
    engine: HashMap<u32, f32>,
    /// Dedicated memory in bytes.
    memory: HashMap<u32, u64>,
}

/// A performance counter query over every GPU engine and every process's
/// GPU memory.
struct GpuCounters {
    query: PDH_HQUERY,
    engine: PDH_HCOUNTER,
    memory: PDH_HCOUNTER,
}

impl GpuCounters {
    /// Opens the query, or returns `None` when this machine has no GPU
    /// counters.
    fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` is a writable out-pointer, and a null data source
        // selects live data.
        if unsafe { PdhOpenQueryW(None, 0, &mut query) } != 0 {
            return None;
        }
        // Owning the query right away closes it on every early return.
        let mut counters = Self {
            query,
            engine: PDH_HCOUNTER::default(),
            memory: PDH_HCOUNTER::default(),
        };
        // English names work whatever the display language of Windows.
        // SAFETY: `query` is open and owned by `counters`, the paths are
        // NUL-terminated literals, and the out-pointers are fields of
        // `counters`.
        let added = unsafe {
            PdhAddEnglishCounterW(
                query,
                w!(r"\GPU Engine(*)\Utilization Percentage"),
                0,
                &mut counters.engine,
            ) == 0
                && PdhAddEnglishCounterW(
                    query,
                    w!(r"\GPU Process Memory(*)\Dedicated Usage"),
                    0,
                    &mut counters.memory,
                ) == 0
        };
        added.then_some(counters)
    }

    /// Reads every counter instance, or returns an empty reading when the
    /// collection fails.
    fn collect(&self) -> GpuReading {
        let mut reading = GpuReading::default();
        // SAFETY: `self.query` stays open for as long as `self` lives.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return reading;
        }
        // Engine instances look like
        // pid_1234_luid_0x0_0x1_phys_0_eng_3_engtype_3D. Like Task Manager,
        // add up the engines of each type and report the busiest type, so
        // a process saturating the copy engine shows as busy too.
        let mut by_type: HashMap<(u32, String), f64> = HashMap::new();
        for (name, value) in
            counter_values(self.engine, PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100))
        {
            let Some(pid) = instance_pid(&name) else {
                continue;
            };
            let kind = name
                .split_once("engtype_")
                .map_or("", |(_, t)| t)
                .to_owned();
            // The counter dips slightly below zero now and then, which would
            // show as "-0%".
            *by_type.entry((pid, kind)).or_default() += value.max(0.0);
        }
        for ((pid, _), value) in by_type {
            let busiest = reading.engine.entry(pid).or_default();
            *busiest = busiest.max(value.min(100.0) as f32);
        }
        // Memory instances look like pid_1234_luid_0x0_0x1_phys_0, one per
        // adapter.
        for (name, value) in counter_values(self.memory, PDH_FMT_DOUBLE) {
            if let Some(pid) = instance_pid(&name) {
                *reading.memory.entry(pid).or_default() += value as u64;
            }
        }
        reading
    }
}

impl Drop for GpuCounters {
    fn drop(&mut self) {
        // SAFETY: `self.query` is owned by this value and closed only here.
        // Closing it frees its counters too, which are not used after.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// Returns the instance names and values of a wildcard counter, skipping
/// instances without valid data, as rate counters have on their first
/// collection.
fn counter_values(counter: PDH_HCOUNTER, format: PDH_FMT) -> Vec<(String, f64)> {
    let mut size = 0u32;
    let mut count = 0u32;
    // SAFETY: without a buffer, PDH only writes the size and count it
    // needs to the two out-pointers.
    let status =
        unsafe { PdhGetFormattedCounterArrayW(counter, format, &mut size, &mut count, None) };
    if status != PDH_MORE_DATA {
        return Vec::new();
    }
    // u64 elements keep the buffer aligned for the items' f64 values.
    let mut buffer = vec![0u64; (size as usize).div_ceil(size_of::<u64>())];
    let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    // SAFETY: the buffer holds at least the `size` bytes PDH asked for and
    // is aligned for the items.
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, format, &mut size, &mut count, Some(items))
    };
    if status != 0 {
        return Vec::new();
    }
    // SAFETY: PDH wrote `count` items to the start of the buffer, followed
    // by the name strings they point to, which live as long as the buffer.
    let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
    items
        .iter()
        .filter(|item| {
            matches!(
                item.FmtValue.CStatus,
                PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
            )
        })
        .filter_map(|item| {
            // SAFETY: a NUL-terminated name inside the buffer; the value
            // is a double, as asked for with PDH_FMT_DOUBLE.
            unsafe {
                let name = item.szName.to_string().ok()?;
                Some((name, item.FmtValue.Anonymous.doubleValue))
            }
        })
        .collect()
}

/// Returns the process id a GPU counter instance belongs to.
fn instance_pid(name: &str) -> Option<u32> {
    name.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_process_id_of_gpu_instances() {
        assert_eq!(
            instance_pid("pid_1234_luid_0x00000000_0x0000D1B4_phys_0_eng_0_engtype_3D"),
            Some(1234)
        );
        assert_eq!(instance_pid("pid_88_luid_0x0_0x1_phys_0"), Some(88));
        assert_eq!(instance_pid("_Total"), None);
    }

    #[test]
    fn measures_this_process() {
        let mut sampler = Sampler::new();
        sampler.sample();
        assert!(sampler.sample().memory > 0);
        assert!(private_bytes().is_some_and(|b| b > 0));
    }
}
