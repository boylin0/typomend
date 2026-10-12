// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Helpers shared by the tool windows opened from the settings window.
//!
//! Each tool window is a modeless dialog of its own rather than a property
//! page, so it can stay open beside the settings window or without it. The
//! window procedures live in the individual windows; this module holds only
//! what they have in common.

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateDialogParamW, DLGPROC, DestroyWindow, ICON_BIG, ICON_SMALL, IsDialogMessageW, MSG,
    SetWindowTextW, WM_SETICON,
};
use windows::core::{HSTRING, PCWSTR};

use super::controls;
use crate::{icon, platform};

/// Creates the dialog `template` with `proc` and brings it to the front.
///
/// Logs an error and opens nothing if the dialog cannot be created.
pub fn open(template: u16, proc: DLGPROC) {
    // SAFETY: GetModuleHandleW(None) takes no pointer and returns the handle
    // of the executable, which stays loaded for the life of the process.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
        .map(Into::into)
        .unwrap_or_default();
    // SAFETY: the template argument is a MAKEINTRESOURCE value, an integer id
    // passed in place of a string pointer, which the call accepts. The dialog
    // procedure is a function item and so lives for the whole program.
    let created = unsafe {
        CreateDialogParamW(
            Some(instance),
            PCWSTR(usize::from(template) as *const u16),
            None,
            proc,
            LPARAM(0),
        )
    };
    match created {
        Ok(window) => platform::bring_to_front(window),
        Err(e) => tracing::error!(template, "opening a window failed: {e}"),
    }
}

/// Gives the program icon to a window just created, for its title bar,
/// Alt+Tab and the taskbar.
pub fn set_icons(hwnd: HWND) {
    controls::send(
        hwnd,
        WM_SETICON,
        ICON_SMALL as usize,
        icon::app(false).0 as isize,
    );
    controls::send(
        hwnd,
        WM_SETICON,
        ICON_BIG as usize,
        icon::app(true).0 as isize,
    );
}

/// Sets the title bar text of `hwnd`.
pub fn set_title(hwnd: HWND, title: &str) {
    // SAFETY: the HSTRING is null-terminated and outlives the synchronous
    // call, and an invalid window only makes the call fail.
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(title));
    }
}

/// Destroys the tool window `hwnd`.
///
/// The caller must forget the handle afterwards, because it is no longer
/// valid.
pub fn close(hwnd: HWND) {
    // SAFETY: `hwnd` is a window created by this program on the UI thread,
    // which is the thread that calls this, as DestroyWindow requires.
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// Gives `window` a chance to handle `msg`, for Tab, Enter and Esc. Returns
/// whether it did, in which case the message must not be translated or
/// dispatched.
pub fn handle_message(window: Option<HWND>, msg: &MSG) -> bool {
    // SAFETY: `window` is a dialog created by this program, and `msg` is a
    // valid reference that outlives the call.
    window.is_some_and(|window| unsafe { IsDialogMessageW(window, msg) }.as_bool())
}
