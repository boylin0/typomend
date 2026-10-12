// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The rule list file, `%APPDATA%\typomend\rules.txt`.
//!
//! Rules live in a plain UTF-8 text file so they can be edited in any editor,
//! and in bulk. The file's directory is watched with a change notification
//! handle rather than polled, so a save takes effect at once and an idle
//! program does no work.
//!
//! A new file is always seeded with the built-in rules, so the user starts
//! from a working list instead of an empty one. The file is parsed leniently:
//! lines that are not valid rules are reported in [`RuleList::errors`] and the
//! rest still take effect.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use typomend_engine::{ParseError, Rule, RuleCorrector, parse_rules};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::core::HSTRING;

use crate::config::Config;
use crate::fl;

/// How long to wait after a change notification before reading the file.
///
/// Editors may write a file in several steps, or through a temporary file
/// that is renamed into place. Waiting a moment lets the save finish first.
const SETTLE: Duration = Duration::from_millis(80);

/// The rule list as last read from disk.
#[derive(Debug, Clone, Default)]
pub struct RuleList {
    /// The rules that parsed, in file order.
    pub rules: Arc<Vec<Rule>>,
    /// Lines that are not valid rules.
    pub errors: Vec<ParseError>,
    /// Local time of the last read, as `HH:MM:SS`.
    pub loaded_at: String,
    /// The file does not exist or could not be read.
    pub missing: bool,
}

/// Returns the path of the rule file, or `None` when the configuration
/// directory is unknown.
pub fn path() -> Option<PathBuf> {
    Config::dir().map(|d| d.join("rules.txt"))
}

/// Creates the rule file, seeded with the built-in rules, if it does not
/// exist.
///
/// Does nothing when the configuration directory is unknown or the file
/// already exists, so the user's edits are never overwritten.
///
/// # Errors
///
/// Returns an error when the directory or the file cannot be created.
pub fn ensure_exists() -> std::io::Result<()> {
    let Some(path) = path() else {
        return Ok(());
    };
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = format!(
        "# {}\n# {}\n\n",
        fl!("rules-file-header-format"),
        fl!("rules-file-header-apply")
    );
    for rule in RuleCorrector::builtin_rules() {
        text.push_str(&format!("{} = {}\n", rule.from, rule.to));
    }
    std::fs::write(path, text)
}

/// Reads and parses the rule file.
///
/// A file that cannot be read gives an empty list marked
/// [`RuleList::missing`].
pub fn load() -> RuleList {
    let loaded_at = crate::state::local_time();
    let Some(text) = path().and_then(|p| std::fs::read(p).ok()) else {
        return RuleList {
            loaded_at,
            missing: true,
            ..Default::default()
        };
    };
    // Accept files saved in a legacy code page instead of failing outright;
    // only the affected characters are lost.
    let (rules, errors) = parse_rules(&String::from_utf8_lossy(&text));
    RuleList {
        rules: Arc::new(rules),
        errors,
        loaded_at,
        missing: false,
    }
}

/// Calls `on_change` on a background thread whenever the rule file is
/// created, saved, renamed or deleted.
///
/// Failures are logged. The watcher stops if the directory cannot be
/// watched any longer.
pub fn watch(on_change: impl Fn() + Send + 'static) {
    let Some(file) = path() else { return };
    let Some(dir) = file.parent().map(|d| d.to_path_buf()) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("typomend-rules".into())
        .spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            // SAFETY: the path is a NUL-terminated HSTRING that outlives the
            // call. The returned handle is used by this thread only and
            // closed once, after the loop.
            let handle = match unsafe {
                FindFirstChangeNotificationW(
                    &HSTRING::from(dir.as_os_str()),
                    false,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_LAST_WRITE
                        | FILE_NOTIFY_CHANGE_SIZE,
                )
            } {
                Ok(handle) => handle,
                Err(e) => {
                    tracing::error!("watching {} failed: {e}", dir.display());
                    return;
                }
            };
            let mut last = signature(&file);
            loop {
                // SAFETY: `handle` is a valid change notification handle that
                // stays open until after the loop.
                if unsafe { WaitForSingleObject(handle, INFINITE) } != WAIT_OBJECT_0 {
                    break;
                }
                std::thread::sleep(SETTLE);
                // Other files in the directory, such as config.toml, raise
                // the same notification; act only when this file changed.
                let now = signature(&file);
                if now != last {
                    last = now;
                    on_change();
                }
                // SAFETY: `handle` is a valid change notification handle that
                // stays open until after the loop.
                if unsafe { FindNextChangeNotification(handle) }.is_err() {
                    break;
                }
            }
            close(handle);
        });
    if let Err(e) = spawned {
        tracing::error!("spawning the rule watcher failed: {e}");
    }
}

/// Closes a change notification handle. Such handles are closed with
/// FindCloseChangeNotification, never CloseHandle.
fn close(handle: HANDLE) {
    // SAFETY: the only caller passes a handle from
    // FindFirstChangeNotificationW that it owns, once, after its last use.
    unsafe {
        let _ = FindCloseChangeNotification(handle);
    }
}

/// Returns the size and modification time of `path`, which change on every
/// save, or `None` when the file is missing.
fn signature(path: &std::path::Path) -> Option<(u64, SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}
