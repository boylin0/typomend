// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The UI thread: a hidden message-only window and the message loop.
//!
//! Other threads and callbacks never touch windows directly. They post one of
//! the `WM_APP_*` messages below to the hidden window, and its window
//! procedure acts on the UI thread. A second instance of Typomend finds this
//! window by class name and posts [`WM_APP_SHOW`] to it.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_LINK_CLASS, ICC_LISTVIEW_CLASSES, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES,
    ICC_TAB_CLASSES, ICC_UPDOWN_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
    PostMessageW, RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::dialog;

/// Class name of the hidden window, also used to find a running instance.
pub const CLASS_NAME: PCWSTR = w!("Typomend.Main");

/// Opens the settings window or brings it to the front.
pub const WM_APP_SHOW: u32 = WM_APP + 1;
/// New observer status or configuration; refresh the open settings window.
pub const WM_APP_REFRESH: u32 = WM_APP + 2;
/// The language changed; re-apply every caption.
pub const WM_APP_RELABEL: u32 = WM_APP + 3;
/// A new resource usage sample; redraw the Usage page.
pub const WM_APP_MONITOR: u32 = WM_APP + 4;
/// The taskbar switched between light and dark; redraw the tray icon.
pub const WM_APP_THEME: u32 = WM_APP + 5;

/// Handle of the hidden window, 0 until [`create_main_window`] has run.
static MAIN: AtomicIsize = AtomicIsize::new(0);

/// Posts `msg` to the UI thread. Safe to call from any thread, and a no-op
/// before the UI thread has started.
pub fn post(msg: u32) {
    let raw = MAIN.load(Ordering::Acquire);
    if raw != 0 {
        // SAFETY: a non-zero `MAIN` is the handle of the hidden window, which
        // is never destroyed before exit, and `PostMessageW` may be called
        // from any thread.
        unsafe {
            let _ = PostMessageW(Some(HWND(raw as *mut _)), msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// Creates the hidden window and registers the common controls the settings
/// window uses.
///
/// Must be called on the UI thread, which then owns the window and has to run
/// [`run_message_loop`].
///
/// # Errors
///
/// Returns an error when the module handle or the window cannot be obtained.
pub fn create_main_window() -> windows::core::Result<HWND> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES
            | ICC_LISTVIEW_CLASSES
            | ICC_TAB_CLASSES
            | ICC_UPDOWN_CLASS
            | ICC_LINK_CLASS
            | ICC_PROGRESS_CLASS,
    };
    // SAFETY: every pointer passed points to a local that outlives its call,
    // and the window is created on this thread, which becomes its owner.
    unsafe {
        let _ = InitCommonControlsEx(&controls);
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            &HSTRING::from(crate::platform::WINDOW_TITLE),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )?;
        MAIN.store(hwnd.0 as isize, Ordering::Release);
        Ok(hwnd)
    }
}

/// Runs the message loop until the program quits.
///
/// Messages for the settings window are offered to [`dialog::handle_message`]
/// first, so that its keyboard navigation works.
pub fn run_message_loop() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid `MSG` that outlives the loop, and the loop runs
    // on the thread that owns the windows, as message dispatch requires.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if dialog::handle_message(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Window procedure of the hidden window.
///
/// Acts on the `WM_APP_*` messages and leaves the rest to the default
/// handling.
///
/// # Safety
///
/// Must only be called by Windows, as the window procedure of a window of
/// class [`CLASS_NAME`], with the arguments of the message being delivered.
unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW => dialog::show(),
        WM_APP_REFRESH => dialog::refresh(),
        WM_APP_RELABEL => dialog::relabel(),
        WM_APP_MONITOR => dialog::refresh_monitor(),
        WM_APP_THEME => crate::tray::retheme(),
        // SAFETY: the arguments are those Windows delivered to this procedure,
        // which is what `DefWindowProcW` expects.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    LRESULT(0)
}
