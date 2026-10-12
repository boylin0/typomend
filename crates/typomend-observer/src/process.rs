// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Facts about other processes: their executable name, and whether they run
//! elevated. Windows blocks input from a normal process to an elevated one,
//! so the observer leaves elevated programs alone.

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

/// A process or token handle, closed when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this module, and only this drop
        // closes it.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Opens `pid` for limited queries.
fn open(pid: u32) -> Option<Handle> {
    // SAFETY: OpenProcess takes no pointers; failure comes back as an error.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .ok()
        .map(Handle)
}

/// Returns the executable name of `pid` without extension, such as `chrome`.
pub(crate) fn name(pid: u32) -> Option<String> {
    let process = open(pid)?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` and `len` describe a valid writable buffer.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
}

/// Returns whether `pid` runs elevated. A process that cannot even be
/// queried is treated as elevated, since input sent to it would be blocked
/// as well.
pub(crate) fn is_elevated(pid: u32) -> bool {
    match open(pid) {
        Some(process) => token_elevated(process.0).unwrap_or(true),
        None => true,
    }
}

/// Returns whether this process runs elevated.
pub(crate) fn self_elevated() -> bool {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// Returns whether the token of `process` is elevated, or `None` when it
/// cannot be read.
fn token_elevated(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    // SAFETY: `token` receives a handle that is wrapped and closed below.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let token = Handle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a correctly sized TOKEN_ELEVATION.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }
    .ok()?;
    Some(elevation.TokenIsElevated != 0)
}
