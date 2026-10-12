// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Asks the focused window's IME which mode it is in.
//!
//! Every window has a default IME window that answers WM_IME_CONTROL, even
//! across processes and for TSF input methods such as Microsoft Bopomofo.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    SMTO_ABORTIFHUNG, SendMessageTimeoutW,
};

// Message and values from imm.h.
const WM_IME_CONTROL: u32 = 0x0283;
const IMC_GETCONVERSIONMODE: usize = 0x0001;
const IMC_GETOPENSTATUS: usize = 0x0005;
const IME_CMODE_NATIVE: usize = 0x0001;

/// Returns whether the IME of the focused window is open and in native mode,
/// which for Bopomofo means Chinese rather than alphanumeric input.
///
/// In native mode, keys may be held in an inline composition. When the IME
/// cannot be asked, the answer is `true`, since that is the safe assumption.
pub(crate) fn native_mode() -> bool {
    let Some(ime) = ime_window() else {
        return true;
    };
    let open = query(ime, IMC_GETOPENSTATUS);
    let mode = query(ime, IMC_GETCONVERSIONMODE);
    match (open, mode) {
        (Some(open), Some(mode)) => open != 0 && mode & IME_CMODE_NATIVE != 0,
        _ => true,
    }
}

/// Returns the default IME window of the focused control, or of the
/// foreground window when its thread reports no focus.
fn ime_window() -> Option<HWND> {
    // SAFETY: `info` is a correctly sized GUITHREADINFO, and every window
    // handle passed on comes straight from the system. A window destroyed in
    // between only makes the later calls fail.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return None;
        }
        let thread = GetWindowThreadProcessId(foreground, None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let focus = if GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.is_invalid() {
            info.hwndFocus
        } else {
            foreground
        };
        let ime = ImmGetDefaultIMEWnd(focus);
        (!ime.is_invalid()).then_some(ime)
    }
}

/// Sends the WM_IME_CONTROL query `command` to `ime` and returns the answer.
/// Gives up after 200 ms, so a hung program cannot stall the observer.
fn query(ime: HWND, command: usize) -> Option<usize> {
    let mut result = 0usize;
    // SAFETY: these queries carry no pointers in their parameters, and
    // `result` outlives the synchronous call.
    let ok = unsafe {
        SendMessageTimeoutW(
            ime,
            WM_IME_CONTROL,
            WPARAM(command),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            200,
            Some(&mut result),
        )
    };
    (ok.0 != 0).then_some(result)
}
