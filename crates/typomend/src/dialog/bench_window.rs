// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The accuracy benchmark window, opened from the Masked language model page: types
//! the bundled test sets through the chosen model and mode and shows how many
//! typos it fixed, how much correct text it changed and how long each check
//! took, next to the figures measured when the model was added.
//!
//! The benchmark itself runs in [`crate::lm::Engines`], so it keeps going
//! when the window is closed and shows again when it is reopened. The window
//! holds no benchmark state of its own and only mirrors what the engines
//! report, which [`refresh`] polls.

use std::cell::{Cell, RefCell};

use typomend_scorer::bench::TEST_SETS;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_INITDIALOG,
};

use super::controls::{self, ListView};
use super::ids::*;
use super::pages::{lm_mode_name, measured, set_label};
use super::{state, window as win};
use crate::config::Config;
use crate::lm::{self, BenchState};
use crate::{fl, platform};

const BN_CLICKED: u32 = 0;
const IDCANCEL: i32 = 2;
const PBM_SETPOS: u32 = 0x0400 + 2;
const PBM_SETRANGE32: u32 = 0x0400 + 6;

thread_local! {
    // Handle of the open window, or 0 while none is open.
    static WINDOW: Cell<isize> = const { Cell::new(0) };
    // Rows the list shows, so a refresh only rebuilds it when they change.
    static SHOWN: RefCell<Option<Vec<[String; 5]>>> = const { RefCell::new(None) };
    // Pairs in each test set, in the order of TEST_SETS.
    static SET_SIZES: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Returns the open window, if any.
fn window() -> Option<HWND> {
    let raw = WINDOW.get();
    (raw != 0).then_some(HWND(raw as *mut _))
}

/// Opens the window, or brings it to the front if it is open.
pub fn show() {
    if let Some(window) = window() {
        platform::bring_to_front(window);
        return;
    }
    win::open(IDD_BENCH, Some(proc));
}

/// See [`win::handle_message`].
pub fn handle_message(msg: &MSG) -> bool {
    win::handle_message(window(), msg)
}

/// Shows the progress or result of the benchmark.
pub fn refresh() {
    if let Some(window) = window() {
        update(window, false);
    }
}

/// Re-applies every caption after the language changed.
pub fn relabel() {
    if let Some(window) = window() {
        texts(window);
        update(window, true);
    }
}

/// Dialog procedure of the benchmark window.
///
/// # Safety
///
/// Windows calls it with the arguments of a dialog message for the window it
/// belongs to.
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            WINDOW.set(hwnd.0 as isize);
            win::set_icons(hwnd);
            SET_SIZES.with(|s| {
                *s.borrow_mut() = TEST_SETS.iter().map(|set| set.cases().len()).collect();
            });
            ListView::new(hwnd, IDC_BENCH_LIST).setup(&columns());
            texts(hwnd);
            update(hwnd, true);
            1
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            match (id, code) {
                (IDC_BENCH_RUN, BN_CLICKED) => start_or_stop(),
                (IDCANCEL, _) => win::close(hwnd),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            win::close(hwnd);
            0
        }
        WM_DESTROY => {
            WINDOW.set(0);
            SHOWN.with(|s| *s.borrow_mut() = None);
            0
        }
        _ => 0,
    }
}

/// Sets the captions that depend on the language.
fn texts(hwnd: HWND) {
    win::set_title(hwnd, &fl!("bench-title"));
    controls::set_text(hwnd, IDC_BENCH_INTRO, &fl!("bench-intro"));
    let titles: Vec<String> = columns().into_iter().map(|(t, _)| t).collect();
    ListView::new(hwnd, IDC_BENCH_LIST).set_column_titles(&titles);
}

/// Returns the list columns as titles with width weights.
///
/// Keep in sync with the cells built by [`rows`].
fn columns() -> [(String, u32); 5] {
    [
        (fl!("bench-set"), 3),
        (fl!("bench-fixed"), 2),
        (fl!("bench-changed"), 2),
        (fl!("bench-ms"), 2),
        (fl!("bench-reference"), 3),
    ]
}

/// Cancels the running benchmark, or starts one with the current settings.
fn start_or_stop() {
    let state = state();
    if matches!(
        state.engines.bench_state(),
        Some(BenchState::Running { .. })
    ) {
        state.engines.cancel_bench();
    } else {
        state.engines.start_bench(state.config().lm);
    }
}

/// Brings the controls in line with the benchmark state.
///
/// The Run button is enabled while a benchmark can be stopped, or when the
/// model is installed and not being downloaded. The list is rebuilt only when
/// its rows changed or `force` is set, which avoids flicker on every poll.
fn update(hwnd: HWND, force: bool) {
    let state = state();
    let config = state.config();
    let bench = state.engines.bench_state();
    let running = matches!(bench, Some(BenchState::Running { .. }));
    let downloading = state
        .engines
        .download_state()
        .is_some_and(|d| matches!(d, lm::DownloadState::Running { .. }));
    let installed = lm::is_installed(&config.lm, &config.lm.model);

    controls::set_text(
        hwnd,
        IDC_BENCH_MODEL,
        &fl!(
            "bench-model",
            model = config.lm.model.clone(),
            mode = lm_mode_name(config.lm.mode)
        ),
    );
    let label = if running {
        fl!("lm-bench-stop")
    } else {
        fl!("bench-start")
    };
    controls::set_text(hwnd, IDC_BENCH_RUN, &label);
    controls::enable(hwnd, IDC_BENCH_RUN, running || (installed && !downloading));
    set_progress(hwnd, bench.as_ref());
    controls::set_text(
        hwnd,
        IDC_BENCH_STATUS,
        &status(&config, bench.as_ref(), installed),
    );

    let rows = rows(&config, bench.as_ref());
    let changed = SHOWN.with(|s| {
        let mut shown = s.borrow_mut();
        let changed = force || shown.as_ref() != Some(&rows);
        *shown = Some(rows.clone());
        changed
    });
    if changed {
        let list = ListView::new(hwnd, IDC_BENCH_LIST);
        list.clear();
        for row in &rows {
            list.push(row);
        }
    }
}

/// Fills the progress bar in proportion to the pairs typed over all test
/// sets, since they differ in size.
fn set_progress(hwnd: HWND, bench: Option<&BenchState>) {
    let sizes = SET_SIZES.with(|s| s.borrow().clone());
    let total: usize = sizes.iter().sum();
    let done = match bench {
        Some(BenchState::Running { set, done, .. }) => {
            let before: usize = TEST_SETS
                .iter()
                .zip(&sizes)
                .take_while(|(s, _)| s.id != *set)
                .map(|(_, n)| n)
                .sum();
            before + done
        }
        Some(BenchState::Done { .. }) => total,
        _ => 0,
    };
    let bar = controls::item(hwnd, IDC_BENCH_PROGRESS);
    controls::send(bar, PBM_SETRANGE32, 0, total.max(1) as isize);
    controls::send(bar, PBM_SETPOS, done, 0);
}

/// Returns the status line for the benchmark.
///
/// A failed or finished run is reported only while it still matches the
/// current model and settings; otherwise the line falls back to a hint.
fn status(config: &Config, bench: Option<&BenchState>, installed: bool) -> String {
    match bench {
        Some(BenchState::Running {
            model,
            set,
            done,
            total,
        }) => fl!(
            "lm-status-benchmarking",
            model = model.clone(),
            set = set_label(set),
            done = done.to_string(),
            total = total.to_string()
        ),
        Some(BenchState::Failed { model, error }) if *model == config.lm.model => {
            fl!("lm-bench-failed", error = error.clone())
        }
        Some(BenchState::Done { config: ran, .. }) if *ran == config.lm => fl!("bench-done"),
        _ if !installed => fl!("bench-not-installed"),
        _ => fl!("bench-hint"),
    }
}

/// One row per test set: the result measured here with the current
/// settings, if any, and the figures measured when the model was added.
fn rows(config: &Config, bench: Option<&BenchState>) -> Vec<[String; 5]> {
    let here = match bench {
        Some(BenchState::Done {
            config: ran,
            results,
        }) if *ran == config.lm => Some(results),
        _ => None,
    };
    let reference = measured(&config.lm.model, config.lm.mode);
    TEST_SETS
        .iter()
        .enumerate()
        .map(|(i, set)| {
            let outcome = here.and_then(|r| r.iter().find(|(id, _)| *id == set.id));
            let (fixed, changed, ms) = match outcome {
                Some((_, o)) => (
                    format!("{:.0}%", o.fixed_rate() * 100.0),
                    format!("{:.0}%", o.false_positive_rate() * 100.0),
                    format!("{} ms", o.mean_call().as_millis()),
                ),
                None => ("-".into(), "-".into(), "-".into()),
            };
            let reference = reference.and_then(|m| m.get(i)).map_or_else(
                || "-".to_owned(),
                |m| format!("{}% / {}% / {} ms", m.fixed, m.false_positives, m.ms),
            );
            [set_label(set.id), fixed, changed, ms, reference]
        })
        .collect()
}
