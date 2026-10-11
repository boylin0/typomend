// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Log files under `%LOCALAPPDATA%\typomend\logs`, one per day.
//!
//! Logs never contain the text the user typed; the observer only logs program
//! names, support levels and outcomes at the default level.
//!
//! The writer is our own rather than tracing-appender's so that the private
//! mode can stop writing at once and the log files can be deleted while the
//! program runs: the current file is only held open between writes, and every
//! write first checks whether logging is paused.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

use crate::state::local_date;

/// Whether logging is paused by the private mode.
static PRIVATE: AtomicBool = AtomicBool::new(false);

/// The open log file with the date it belongs to, `None` while none is open.
static FILE: Mutex<Option<(String, File)>> = Mutex::new(None);

/// Returns the directory of the log files, or `None` when the platform has no
/// local data directory.
pub fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("typomend").join("logs"))
}

/// Starts logging.
///
/// `directives` chooses the log lines, as comma-separated `target=level`
/// directives such as `typomend_observer=debug,info`, and `TYPOMEND_LOG`
/// overrides it. Without either, or when they do not parse, everything at
/// info level and above is written. A plain target filter is used instead of
/// `EnvFilter` because the latter pulls a regex engine into the binary.
pub fn init(private: bool, directives: Option<&str>) {
    set_private(private);
    let directives = std::env::var("TYPOMEND_LOG")
        .ok()
        .or(directives.map(str::to_owned));
    let parsed = directives.as_deref().map(str::parse::<Targets>);
    let filter = match &parsed {
        Some(Ok(targets)) => targets.clone(),
        _ => Targets::new().with_default(LevelFilter::INFO),
    };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(LogFiles)
                .with_ansi(false),
        )
        .with(filter)
        .init();
    if let Some(Err(e)) = parsed {
        tracing::warn!("ignoring the log directives {directives:?}: {e}");
    }
}

/// Stops or resumes writing log files. While stopped, log lines are dropped,
/// not buffered.
pub fn set_private(private: bool) {
    PRIVATE.store(private, Ordering::Release);
    if private {
        // Let go of the file so it can be deleted.
        *FILE.lock() = None;
    }
}

/// Deletes every log file and returns how many there were.
///
/// # Errors
///
/// Returns an error when the log directory cannot be read or a file cannot
/// be removed. Files after the failing one are left in place.
pub fn delete_all() -> io::Result<usize> {
    let Some(dir) = dir() else {
        return Ok(0);
    };
    // Holding the lock keeps a concurrent write from reopening a file that is
    // about to be deleted.
    let mut file = FILE.lock();
    *file = None;
    let mut deleted = 0;
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        if name.as_deref().is_some_and(is_log_file) {
            std::fs::remove_file(&path)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Matches the `typomend.<date>.log` names the log files are given.
fn is_log_file(name: &str) -> bool {
    name.starts_with("typomend.") && name.ends_with(".log")
}

/// Hands out [`LogWriter`]s to the tracing subscriber.
struct LogFiles;

impl<'a> MakeWriter<'a> for LogFiles {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter
    }
}

/// Appends to today's log file, opening it on first use and again after the
/// date changes.
struct LogWriter;

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if PRIVATE.load(Ordering::Acquire) {
            return Ok(buf.len());
        }
        let mut current = FILE.lock();
        let today = local_date();
        if current.as_ref().is_none_or(|(date, _)| *date != today) {
            let Some(dir) = dir() else {
                return Ok(buf.len());
            };
            std::fs::create_dir_all(&dir)?;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(format!("typomend.{today}.log")))?;
            *current = Some((today, file));
        }
        match current.as_mut() {
            Some((_, file)) => file.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match FILE.lock().as_mut() {
            Some((_, file)) => file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_log_names() {
        assert!(is_log_file("typomend.2026-10-06.log"));
        assert!(!is_log_file("rules.txt"));
        assert!(!is_log_file("other.log"));
    }
}
