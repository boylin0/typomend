// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The program and notification area icons.
//!
//! Both come from the icon files in `resources/`, with a frame for every size
//! Windows asks for. The program icon is the brand's app icon: the symbol on
//! an ink tile. The notification
//! area shows the bare monochrome symbol instead, which the brand guidelines
//! prefer at 16 to 32 pixels. It is ink on a light taskbar and white on a
//! dark one, and dimmed while corrections are paused.

use std::cell::Cell;

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD, RegCloseKey,
    RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows::Win32::UI::Controls::LoadIconWithScaleDown;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, HICON, SM_CXICON, SM_CXSMICON};
use windows::core::{PCWSTR, w};

// Icon resource ids. Keep in sync with `resources/resource.h`. The program
// icon has the lowest id because Explorer shows the first icon in the
// executable.
pub const IDI_APP: u16 = 1;
const IDI_TRAY_INK: u16 = 2;
const IDI_TRAY_INK_PAUSED: u16 = 3;
const IDI_TRAY_WHITE: u16 = 4;
const IDI_TRAY_WHITE_PAUSED: u16 = 5;

const PERSONALIZE: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");

thread_local! {
    // Loaded once and kept until exit, as windows keep using the handles.
    static SMALL: Cell<Option<HICON>> = const { Cell::new(None) };
    static LARGE: Cell<Option<HICON>> = const { Cell::new(None) };
}

/// Returns the program icon at the system's small icon size, for title
/// bars, or at its large icon size, for Alt+Tab and the taskbar.
///
/// The icon is loaded on first use and cached per thread. Callers must not
/// destroy it. Returns a null handle when loading fails.
pub fn app(large: bool) -> HICON {
    let (slot, metric) = if large {
        (&LARGE, SM_CXICON)
    } else {
        (&SMALL, SM_CXSMICON)
    };
    slot.with(|cell| {
        if let Some(icon) = cell.get() {
            return icon;
        }
        // SAFETY: `GetSystemMetrics` takes a plain index and has no
        // preconditions.
        let size = unsafe { GetSystemMetrics(metric) };
        let icon = load(IDI_APP, size).unwrap_or_default();
        cell.set(Some(icon));
        icon
    })
}

/// Loads the icon resource `id` at `size` pixels. The caller owns the
/// handle and destroys it with `DestroyIcon`.
pub fn load(id: u16, size: i32) -> windows::core::Result<HICON> {
    // SAFETY: the instance is the handle of this module. The id is passed as
    // an integer resource identifier, a pointer value with only the low word
    // set, which `LoadIconWithScaleDown` accepts in place of a name.
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        LoadIconWithScaleDown(
            Some(instance),
            PCWSTR(usize::from(id) as *const u16),
            size,
            size,
        )
    }
}

/// Returns the notification area icon resource for the current taskbar
/// theme, and its size in pixels.
///
/// The dimmed variant is chosen unless `active` is set. The size is the
/// system's small icon size, at least 16.
pub fn tray(active: bool) -> (u16, u32) {
    let id = match (taskbar_is_light(), active) {
        (true, true) => IDI_TRAY_INK,
        (true, false) => IDI_TRAY_INK_PAUSED,
        (false, true) => IDI_TRAY_WHITE,
        (false, false) => IDI_TRAY_WHITE_PAUSED,
    };
    // SAFETY: `GetSystemMetrics` takes a plain index and has no
    // preconditions.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    (id, size.max(16) as u32)
}

/// Returns whether the taskbar uses the light theme.
///
/// Returns `false` when the registry value is missing, as on Windows versions
/// that predate the setting and always had a dark taskbar.
fn taskbar_is_light() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` is a live `u32` and `size` declares its byte length, so
    // the call cannot write past it.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    read.is_ok() && value != 0
}

/// Calls `on_change` on a background thread whenever the taskbar switches
/// between the light and dark theme.
///
/// The UI thread only has a message-only window, which never receives the
/// WM_SETTINGCHANGE broadcast, so this watches the registry value instead.
pub fn watch_theme(on_change: impl Fn() + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("typomend-theme".into())
        .spawn(move || {
            let mut key = HKEY::default();
            // SAFETY: `key` is a live local that receives the handle, and the
            // key path is a static string.
            let opened = unsafe {
                RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE, None, KEY_NOTIFY, &mut key)
            };
            if let Err(e) = opened.ok() {
                tracing::warn!("watching the taskbar theme failed: {e}");
                return;
            }
            let mut light = taskbar_is_light();
            loop {
                // SAFETY: `key` was opened above with `KEY_NOTIFY` and is not
                // closed until the loop ends.
                let changed = unsafe {
                    RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false)
                };
                if changed.is_err() {
                    break;
                }
                // Other values under the key change too, such as the accent
                // color; only a different taskbar theme needs a new icon.
                let now = taskbar_is_light();
                if now != light {
                    light = now;
                    on_change();
                }
            }
            // SAFETY: `key` was opened successfully above and is not used
            // after this call.
            unsafe {
                let _ = RegCloseKey(key);
            }
        });
    if let Err(e) = spawned {
        tracing::error!("starting the theme watcher failed: {e}");
    }
}
