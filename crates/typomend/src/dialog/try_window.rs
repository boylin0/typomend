// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Try window, opened from the Masked language model page: type some text and
//! see what the correctors in effect make of it.
//!
//! The observer never corrects Typomend's own windows, so text typed here is
//! left alone and the result shows only what the check found.
//!
//! The check runs in [`crate::lm::Engines`] off the UI thread. The window
//! holds no result of its own and only mirrors what the engines report, which
//! [`refresh`] polls.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_INITDIALOG,
};

use super::controls::{self, ListView};
use super::ids::*;
use super::{state, window as win};
use crate::lm::{TryResult, TryState};
use crate::{fl, platform};

const BN_CLICKED: u32 = 0;
const IDCANCEL: i32 = 2;

thread_local! {
    // Handle of the open window, or 0 while none is open.
    static WINDOW: Cell<isize> = const { Cell::new(0) };
    // Rows the list shows, so a refresh only rebuilds it when they change.
    static SHOWN: RefCell<Option<Vec<[String; 5]>>> = const { RefCell::new(None) };
}

/// Returns the open window, if any.
fn window() -> Option<HWND> {
    let raw = WINDOW.get();
    (raw != 0).then_some(HWND(raw as *mut _))
}

/// Opens the Try window, or brings it to the front if it is open.
pub fn show() {
    if let Some(window) = window() {
        platform::bring_to_front(window);
        return;
    }
    win::open(IDD_TRY, Some(proc));
}

/// Gives the window a chance to handle `msg`, for Tab, Enter and Esc.
/// Returns whether it did, in which case the message must not be translated
/// or dispatched.
pub fn handle_message(msg: &MSG) -> bool {
    win::handle_message(window(), msg)
}

/// Shows the state of the last check.
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

/// Dialog procedure of the Try window.
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
            ListView::new(hwnd, IDC_TRY_LIST).setup(&columns());
            texts(hwnd);
            update(hwnd, true);
            1
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            match (id, code) {
                // The Check button is the default one, so Enter in the text
                // box clicks it too.
                (IDC_TRY_RUN, BN_CLICKED) => start(hwnd),
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
    win::set_title(hwnd, &fl!("try-title"));
    controls::set_text(hwnd, IDC_TRY_INTRO, &fl!("try-intro"));
    controls::set_text(hwnd, IDC_TRY_RUN, &fl!("try-run"));
    let titles: Vec<String> = columns().into_iter().map(|(t, _)| t).collect();
    ListView::new(hwnd, IDC_TRY_LIST).set_column_titles(&titles);
}

/// Returns the list columns as titles with width weights.
///
/// Keep in sync with the cells built by [`rows`].
fn columns() -> [(String, u32); 5] {
    [
        (fl!("try-char"), 2),
        (fl!("try-best"), 2),
        (fl!("try-ratio"), 2),
        (fl!("try-required"), 2),
        (fl!("try-verdict"), 4),
    ]
}

/// Starts a check of the text in the box, ignoring blank text.
fn start(hwnd: HWND) {
    let text = controls::text(hwnd, IDC_TRY_TEXT);
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let state = state();
    let within = state.config().caret_window;
    state.engines.start_try(text.to_owned(), within);
}

/// Brings the controls in line with the state of the last check.
///
/// The list is rebuilt only when its rows changed or `force` is set, which
/// avoids flicker on every poll.
fn update(hwnd: HWND, force: bool) {
    let try_state = state().engines.try_state();
    let running = matches!(try_state, Some(TryState::Running));
    controls::enable(hwnd, IDC_TRY_RUN, !running);
    let (summary, rows) = match &try_state {
        None => (fl!("try-hint"), Vec::new()),
        Some(TryState::Running) => (fl!("try-running"), Vec::new()),
        Some(TryState::Failed(error)) => (fl!("try-failed", error = error.clone()), Vec::new()),
        Some(TryState::Done(result)) => (summary(result), rows(result)),
    };
    controls::set_text(hwnd, IDC_TRY_RESULT, &summary);
    // While a check runs, the last result stays in the list.
    if running {
        return;
    }
    let changed = SHOWN.with(|s| {
        let mut shown = s.borrow_mut();
        let changed = force || shown.as_ref() != Some(&rows);
        *shown = Some(rows.clone());
        changed
    });
    if changed {
        let list = ListView::new(hwnd, IDC_TRY_LIST);
        list.clear();
        for row in &rows {
            list.push(row);
        }
    }
}

/// Returns the text that summarizes `result`, one fact per line.
fn summary(result: &TryResult) -> String {
    let outcome = |text: &str| {
        if text == result.text {
            fl!("try-unchanged", text = text.to_owned())
        } else {
            text.to_owned()
        }
    };
    let fixes: Vec<String> = result
        .fixes
        .iter()
        .map(|f| format!("{} → {}", f.from, f.to))
        .collect();
    let mut committed = outcome(&result.committed);
    if !fixes.is_empty() {
        committed = fl!(
            "try-with-fixes",
            text = committed,
            fixes = fixes.join(" / ")
        );
    }
    let ms = result.elapsed.as_millis().to_string();
    let last = if result.verdicts.is_some() {
        fl!("try-explained", ms = ms)
    } else {
        fl!("try-no-model", ms = ms)
    };
    [
        fl!("try-committed", text = committed),
        fl!("try-typed", text = outcome(&result.typed)),
        last,
    ]
    .join("\n")
}

/// Returns one list row per assessed character, empty when no model ran.
fn rows(result: &TryResult) -> Vec<[String; 5]> {
    let Some(verdicts) = &result.verdicts else {
        return Vec::new();
    };
    verdicts
        .iter()
        .map(|v| {
            let a = &v.assessment;
            let verdict = match (v.passes, v.ended) {
                (true, None) => fl!("try-passes"),
                (false, None) => fl!("try-fails"),
                (true, Some(e)) => fl!("try-passes-ended", ratio = ratio(e.ratio)),
                (false, Some(e)) => fl!("try-fails-ended", ratio = ratio(e.ratio)),
            };
            [
                format!("{} {}", a.position + 1, a.original),
                a.best.to_string(),
                ratio(a.ratio),
                ratio(v.required),
                verdict,
            ]
        })
        .collect()
}

/// Formats a likelihood ratio, with fewer digits the larger it is.
fn ratio(r: f32) -> String {
    if r >= 100.0 {
        format!("{r:.0}")
    } else if r >= 0.01 {
        format!("{r:.2}")
    } else {
        "< 0.01".to_owned()
    }
}
