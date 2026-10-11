// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Thin wrappers over the Win32 calls the rest of the program needs: the
//! single-instance mutex, window activation, message boxes, the autostart
//! entry in the registry, the folder picker and opening files, folders and
//! links through the shell.
//!
//! Each wrapper hides the `unsafe` call behind a safe signature and states why
//! the call is sound. Failures the user can see for themselves, such as a link
//! that does not open, are ignored rather than reported.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowExW, GA_ROOT, GetAncestor, HWND_MESSAGE, IDYES,
    MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MessageBoxW, PostMessageW, SW_RESTORE,
    SW_SHOW, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::app;

/// Title of the settings window and of message boxes.
pub const WINDOW_TITLE: &str = "Typomend";

/// Registry key, under HKEY_CURRENT_USER, whose values Windows runs at sign
/// in.
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
/// Name of the value under [`RUN_KEY`] that starts Typomend.
const RUN_VALUE: PCWSTR = w!("Typomend");

/// Holds the single-instance mutex. The handle is never closed, so the
/// mutex lives until the process exits.
#[derive(Debug)]
pub struct InstanceGuard(#[allow(dead_code)] HANDLE);

/// Claims the single-instance mutex. Returns `None` when another instance is
/// already running in this session.
///
/// A mutex that exists but cannot be opened, as when the running instance is
/// elevated and this one is not, also means another instance is running.
pub fn claim_single_instance() -> Option<InstanceGuard> {
    // SAFETY: the name is a NUL-terminated literal and no security
    // attributes are passed. The handle is moved into the guard.
    let handle = unsafe { CreateMutexW(None, false, w!(r"Local\Typomend.SingleInstance")) }.ok()?;
    // SAFETY: GetLastError has no preconditions. Nothing runs between it
    // and CreateMutexW, so it reports that call.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(InstanceGuard(handle))
}

/// Asks the running instance to open its settings window.
pub fn show_running_instance() {
    // Windows lets only the foreground process bring a window to the front.
    // This process holds that right because the user just started it, and
    // hands it on so the running instance can show its window.
    // SAFETY: the class name is a NUL-terminated literal, and the posted
    // message carries no pointers.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        if let Ok(hwnd) = FindWindowExW(Some(HWND_MESSAGE), None, app::CLASS_NAME, None) {
            let _ = PostMessageW(Some(hwnd), app::WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}

/// Shows an informational message box owned by the window containing
/// `child`, titled with the program name. A null `child` gives a box with no
/// owner.
pub fn message_box(child: HWND, text: &str) {
    // SAFETY: both calls accept any window handle, including a stale or
    // null one, and the temporary HSTRINGs are NUL-terminated and outlive the
    // modal call.
    unsafe {
        let owner = GetAncestor(child, GA_ROOT);
        MessageBoxW(
            Some(owner),
            &HSTRING::from(text),
            &HSTRING::from(WINDOW_TITLE),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Shows `hwnd`, restores it if minimized, and activates it.
pub fn bring_to_front(hwnd: HWND) {
    // SAFETY: these calls take no pointers, and on a stale or null handle
    // they fail without side effects.
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Returns whether Typomend starts with Windows.
pub fn autostart_enabled() -> bool {
    let mut size = 0u32;
    // SAFETY: the key and value names are NUL-terminated literals. With no
    // data buffer, only the size is written, to `size`.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    }
    .is_ok()
}

/// Adds or removes the Run entry that starts Typomend with Windows.
///
/// The entry starts the program with `--background`, so it stays in the
/// notification area without a window.
///
/// # Errors
///
/// Returns an error when the path of the executable is unknown or the
/// registry refuses the change. Removing an entry that does not exist
/// succeeds.
pub fn set_autostart(enabled: bool) -> windows::core::Result<()> {
    if !enabled {
        // SAFETY: the key and value names are NUL-terminated literals.
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE) };
        // Error 2 is ERROR_FILE_NOT_FOUND: the entry was not there.
        return if result.is_ok() || result.0 == 2 {
            Ok(())
        } else {
            result.ok()
        };
    }
    let exe = std::env::current_exe()
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    let command = format!("\"{}\" --background", exe.display());
    let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated REG_SZ payload that outlives the
    // call, and the size passed is its length in bytes.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
    }
    .ok()
}

/// Asks a yes or no question in a message box owned by the window containing
/// `child`. Returns whether the user answered yes.
pub fn confirm(child: HWND, text: &str) -> bool {
    // SAFETY: both calls accept any window handle, including a stale or
    // null one, and the temporary HSTRINGs are NUL-terminated and outlive the
    // modal call.
    unsafe {
        let owner = GetAncestor(child, GA_ROOT);
        MessageBoxW(
            Some(owner),
            &HSTRING::from(text),
            &HSTRING::from(WINDOW_TITLE),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

/// Lets the user pick a folder, starting in `initial` when it is an existing
/// directory.
///
/// The dialog is owned by the window containing `child`. Returns `None` when
/// the dialog is cancelled or cannot be shown.
pub fn pick_folder(child: HWND, initial: Option<&Path>) -> Option<PathBuf> {
    // SAFETY: COM is initialized on this thread before the dialog is
    // created, and the dialog is used on this thread only. The display name
    // is a CoTaskMemAlloc buffer, copied before it is freed once with
    // CoTaskMemFree.
    unsafe {
        // The UI thread has usually initialized COM already, which
        // CoInitializeEx reports with S_FALSE and is harmless.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;
        if let Some(dir) = initial.filter(|d| d.is_dir()) {
            let item: Option<IShellItem> =
                SHCreateItemFromParsingName(&HSTRING::from(dir.as_os_str()), None).ok();
            if let Some(item) = item {
                let _ = dialog.SetFolder(&item);
            }
        }
        dialog.Show(Some(GetAncestor(child, GA_ROOT))).ok()?;
        let name = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0 as *const _));
        path
    }
}

/// Opens `path` in File Explorer, creating the folder first if it does not
/// exist.
pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    shell_open(&HSTRING::from(path.as_os_str()));
}

/// Opens `path` with the program associated with its file type.
pub fn open_file(path: &Path) {
    shell_open(&HSTRING::from(path.as_os_str()));
}

/// Opens `url` in the default browser.
pub fn open_url(url: &str) {
    shell_open(&HSTRING::from(url));
}

/// Opens `target` the way double-clicking it would.
///
/// Failures are ignored because the user sees that nothing opened.
fn shell_open(target: &HSTRING) {
    // SAFETY: the verb and target are NUL-terminated strings that outlive
    // the call, and ShellExecuteW only reads them.
    unsafe {
        ShellExecuteW(None, w!("open"), target, None, None, SW_SHOWNORMAL);
    }
}
