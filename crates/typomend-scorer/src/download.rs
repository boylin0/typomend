// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Downloads a model from Hugging Face into a directory the scorer can load.
//!
//! The weights are a few hundred megabytes, so they are fetched in fixed
//! chunks by several threads at once with HTTP range requests. Chunks are
//! written in place into a preallocated `model.safetensors.part`, and every
//! finished chunk is recorded in `model.safetensors.chunks`. A cancelled or
//! failed download therefore resumes where it stopped, losing at most the
//! chunks that were in flight. A server that ignores range requests gets a
//! single plain request instead, which cannot resume.
//!
//! The weights are renamed to `model.safetensors` only when complete, and
//! fetched after the small files, so a directory holding that file always
//! holds a whole model.
//!
//! Chunks are written with positioned writes, so threads share one file handle
//! without depending on its cursor.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::{Error, ModelInfo};

/// Files fetched whole before the weights.
const SMALL_FILES: [&str; 2] = ["config.json", "vocab.txt"];

/// Name of the weights file, which exists only once fully downloaded.
const WEIGHTS: &str = "model.safetensors";

/// Bytes per range request.
const CHUNK: u64 = 8 << 20;

/// Most chunks fetched at once.
const THREADS: usize = 4;

/// Times one chunk is tried before the download fails.
const ATTEMPTS: usize = 3;

/// Interval between progress reports.
const TICK: Duration = Duration::from_millis(250);

/// Seconds over which the reported speed is averaged.
const SPEED_WINDOW: f64 = 3.0;

/// Where a download stands, reported a few times per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// Bytes of the weights on disk, including those from an earlier,
    /// interrupted attempt.
    pub done: u64,
    /// Size of the weights, when the server tells.
    pub total: Option<u64>,
    /// Recent speed in bytes per second.
    pub speed: f64,
    /// Connections in use: 1 when the server does not support ranges.
    pub threads: usize,
}

/// Downloads `model` into `dir`, creating it if needed, and resuming an
/// earlier attempt when one was interrupted.
///
/// Files already in `dir` are not fetched again, so calling this for a model
/// that is complete returns at once.
///
/// `progress` is called on the calling thread a few times per second and
/// stops the download by returning `false`; what was fetched so far is kept.
///
/// # Errors
///
/// Returns [`Error::Http`] when a request keeps failing, [`Error::Cancelled`]
/// when `progress` stopped it, and [`Error::Io`] when a file cannot be
/// written.
pub fn download(
    model: &ModelInfo,
    dir: &Path,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<(), Error> {
    fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let agent = agent();
    let url = |name: &str| {
        format!(
            "https://huggingface.co/{}/resolve/{}/{name}",
            model.repo,
            model.revision.replace('/', "%2F")
        )
    };

    for name in SMALL_FILES {
        let target = dir.join(name);
        if !target.is_file() {
            let mut response = agent.get(url(name)).call().map_err(http)?;
            let mut body = Vec::new();
            response
                .body_mut()
                .as_reader()
                .read_to_end(&mut body)
                .map_err(|e| Error::Http(e.to_string()))?;
            write_atomically(&target, &body)?;
        }
    }

    let target = dir.join(WEIGHTS);
    if target.is_file() {
        return Ok(());
    }
    let url = url(WEIGHTS);
    match probe(&agent, &url)? {
        Some(total) => ranged(&agent, &url, dir, total, progress)?,
        None => single(&agent, &url, dir, progress)?,
    }
    fs::rename(dir.join(format!("{WEIGHTS}.part")), &target).map_err(|e| io(&target, e))?;
    let _ = fs::remove_file(dir.join(format!("{WEIGHTS}.chunks")));
    Ok(())
}

/// Returns the size of the file at `url` when the server serves ranges.
fn probe(agent: &ureq::Agent, url: &str) -> Result<Option<u64>, Error> {
    let response = agent
        .get(url)
        .header("Range", "bytes=0-0")
        .call()
        .map_err(http)?;
    if response.status() != 206 {
        return Ok(None);
    }
    // The total follows the slash, as in `Content-Range: bytes 0-0/411578458`.
    Ok(response
        .headers()
        .get("Content-Range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse().ok()))
}

/// Shared state of the chunk workers.
struct Job {
    url: String,
    /// The preallocated `.part` file every worker writes into.
    file: File,
    /// Size of the weights in bytes.
    total: u64,
    /// Next chunk index to hand out, among `pending`.
    next: AtomicUsize,
    /// Chunks not yet on disk.
    pending: Vec<u64>,
    /// Bytes on disk, including chunks from an earlier attempt.
    done: AtomicU64,
    /// The `.chunks` file, appended to as each chunk finishes.
    record: Mutex<File>,
    /// Set to make the workers quit after their current read.
    stop: AtomicBool,
    /// The first error a worker gave up on.
    error: Mutex<Option<Error>>,
}

/// Fetches the weights in chunks with `THREADS` workers, resuming from the
/// chunks recorded by an earlier attempt, and reports to `progress` while it
/// waits for them.
fn ranged(
    agent: &ureq::Agent,
    url: &str,
    dir: &Path,
    total: u64,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<(), Error> {
    let part = dir.join(format!("{WEIGHTS}.part"));
    let record_path = dir.join(format!("{WEIGHTS}.chunks"));
    let chunks = total.div_ceil(CHUNK);

    // The record starts with the total size. A different size means the
    // file changed upstream and the old chunks are useless.
    let finished: BTreeSet<u64> = match fs::read_to_string(&record_path) {
        Ok(text) if part.is_file() && text.lines().next() == Some(&total.to_string()) => text
            .lines()
            .skip(1)
            .filter_map(|l| l.parse().ok())
            .collect(),
        _ => BTreeSet::new(),
    };
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part)
        .map_err(|e| io(&part, e))?;
    file.set_len(total).map_err(|e| io(&part, e))?;
    if finished.is_empty() {
        fs::write(&record_path, format!("{total}\n")).map_err(|e| io(&record_path, e))?;
    }
    let record = OpenOptions::new()
        .append(true)
        .open(&record_path)
        .map_err(|e| io(&record_path, e))?;

    let already: u64 = finished.iter().map(|&c| chunk_len(c, total)).sum();
    let job = Arc::new(Job {
        url: url.to_owned(),
        file,
        total,
        next: AtomicUsize::new(0),
        pending: (0..chunks).filter(|c| !finished.contains(c)).collect(),
        done: AtomicU64::new(already),
        record: Mutex::new(record),
        stop: AtomicBool::new(false),
        error: Mutex::new(None),
    });

    let threads = THREADS.min(job.pending.len()).max(1);
    let workers: Vec<_> = (0..threads)
        .map(|_| {
            let job = Arc::clone(&job);
            let agent = agent.clone();
            thread::spawn(move || work(&agent, &job))
        })
        .collect();

    let mut meter = Meter::new(already);
    let mut cancelled = false;
    while workers.iter().any(|w| !w.is_finished()) {
        thread::sleep(TICK);
        let done = job.done.load(Ordering::Relaxed);
        let report = Progress {
            done,
            total: Some(total),
            speed: meter.update(done),
            threads,
        };
        if !cancelled && !progress(report) {
            cancelled = true;
            job.stop.store(true, Ordering::Relaxed);
        }
    }
    for worker in workers {
        let _ = worker.join();
    }

    if cancelled {
        return Err(Error::Cancelled);
    }
    if let Some(e) = job.error.lock().ok().and_then(|mut e| e.take()) {
        return Err(e);
    }
    job.file.sync_all().map_err(|e| io(&part, e))?;
    Ok(())
}

/// Takes chunks from `job` and fetches them until none are left, `job` is
/// stopped or a chunk fails `ATTEMPTS` times, in which case the error is left
/// in `job` and the other workers are stopped.
fn work(agent: &ureq::Agent, job: &Job) {
    loop {
        if job.stop.load(Ordering::Relaxed) {
            return;
        }
        let i = job.next.fetch_add(1, Ordering::Relaxed);
        let Some(&chunk) = job.pending.get(i) else {
            return;
        };
        let mut result = Ok(());
        for _ in 0..ATTEMPTS {
            result = fetch_chunk(agent, job, chunk);
            if result.is_ok() || job.stop.load(Ordering::Relaxed) {
                break;
            }
        }
        match result {
            Ok(()) => {
                if let Ok(mut record) = job.record.lock() {
                    let _ = writeln!(record, "{chunk}");
                }
            }
            Err(e) => {
                if let Ok(mut error) = job.error.lock() {
                    error.get_or_insert(e);
                }
                job.stop.store(true, Ordering::Relaxed);
                return;
            }
        }
    }
}

/// Fetches one chunk and writes it at its place in the `.part` file.
///
/// On failure, the bytes already counted for the chunk are taken back from
/// `job.done`, so the progress never includes data that must be fetched
/// again.
fn fetch_chunk(agent: &ureq::Agent, job: &Job, chunk: u64) -> Result<(), Error> {
    let start = chunk * CHUNK;
    let len = chunk_len(chunk, job.total);
    let mut response = agent
        .get(&job.url)
        .header("Range", format!("bytes={start}-{}", start + len - 1))
        .call()
        .map_err(http)?;
    if response.status() != 206 {
        return Err(Error::Http(format!(
            "range request answered {}",
            response.status()
        )));
    }
    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0; 1 << 16];
    let mut offset = 0;
    while offset < len {
        if job.stop.load(Ordering::Relaxed) {
            // The chunk is not in the record, so resuming fetches it again.
            job.done.fetch_sub(offset, Ordering::Relaxed);
            return Err(Error::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                job.done.fetch_sub(offset, Ordering::Relaxed);
                return Err(Error::Http(e.to_string()));
            }
        };
        let n = n.min((len - offset) as usize);
        if let Err(e) = write_all_at(&job.file, &buf[..n], start + offset) {
            job.done.fetch_sub(offset, Ordering::Relaxed);
            return Err(Error::Io("model.safetensors.part".into(), e));
        }
        offset += n as u64;
        job.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    if offset != len {
        job.done.fetch_sub(offset, Ordering::Relaxed);
        return Err(Error::Http(format!("chunk {chunk} ended early")));
    }
    Ok(())
}

/// Writes all of `buf` at `offset` without relying on the file cursor.
#[cfg(unix)]
fn write_all_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

/// Writes all of `buf` at `offset`, repeating after short writes.
///
/// `seek_write` moves the file cursor, which nothing here reads.
#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                buf = &buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Fetches the weights in one request, for servers without range support.
fn single(
    agent: &ureq::Agent,
    url: &str,
    dir: &Path,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<(), Error> {
    let part = dir.join(format!("{WEIGHTS}.part"));
    let mut response = agent.get(url).call().map_err(http)?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(&part).map_err(|e| io(&part, e))?;
    let mut buf = vec![0; 1 << 16];
    let mut done = 0;
    let mut meter = Meter::new(0);
    let mut last = Instant::now();
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Http(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| io(&part, e))?;
        done += n as u64;
        if last.elapsed() >= TICK {
            last = Instant::now();
            let report = Progress {
                done,
                total,
                speed: meter.update(done),
                threads: 1,
            };
            if !progress(report) {
                return Err(Error::Cancelled);
            }
        }
    }
    file.sync_all().map_err(|e| io(&part, e))
}

/// Averages the download speed over the last few seconds.
struct Meter {
    /// Byte counts with the time they were taken, within [`SPEED_WINDOW`].
    samples: Vec<(Instant, u64)>,
}

impl Meter {
    /// Creates a meter that starts counting from `done` bytes.
    fn new(done: u64) -> Self {
        Self {
            samples: vec![(Instant::now(), done)],
        }
    }

    /// Records that `done` bytes are on disk now and returns the speed in
    /// bytes per second over the window.
    fn update(&mut self, done: u64) -> f64 {
        let now = Instant::now();
        self.samples.push((now, done));
        self.samples
            .retain(|(t, _)| now.duration_since(*t).as_secs_f64() <= SPEED_WINDOW);
        let (t0, d0) = self.samples[0];
        let secs = now.duration_since(t0).as_secs_f64();
        if secs > 0.0 {
            done.saturating_sub(d0) as f64 / secs
        } else {
            0.0
        }
    }
}

/// Returns the size of chunk number `chunk` of a file of `total` bytes. Only
/// the last chunk can be shorter than [`CHUNK`].
fn chunk_len(chunk: u64, total: u64) -> u64 {
    CHUNK.min(total - chunk * CHUNK)
}

/// Writes `data` to `target` through a `.part` file and a rename, so a
/// reader never sees a half-written file.
fn write_atomically(target: &Path, data: &[u8]) -> Result<(), Error> {
    let mut part = PathBuf::from(target);
    part.as_mut_os_string().push(".part");
    fs::write(&part, data).map_err(|e| io(&part, e))?;
    fs::rename(&part, target).map_err(|e| io(target, e))
}

/// Creates the HTTP agent shared by all requests of a download.
fn agent() -> ureq::Agent {
    // The system TLS library and certificate store, so no TLS library or root
    // certificates ship with Typomend.
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Wraps `e` as an [`Error::Io`] naming `path`.
fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Io(path.display().to_string(), e)
}

/// Wraps a request failure as an [`Error::Http`].
fn http(e: ureq::Error) -> Error {
    Error::Http(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_into_chunks() {
        let total = 2 * CHUNK + 5;
        assert_eq!(total.div_ceil(CHUNK), 3);
        assert_eq!(chunk_len(0, total), CHUNK);
        assert_eq!(chunk_len(2, total), 5);
    }
}
