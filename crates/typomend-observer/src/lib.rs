// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Watches text committed by an IME anywhere on the desktop and writes
//! corrections back into the focused control.
//!
//! The input method is never touched. The observer reads the focused control
//! through UI Automation, follows the IME composition through UIA TextEdit
//! events, and only acts after the composition has been committed. How a
//! control is handled depends on what it exposes, not on which program it
//! belongs to:
//!
//! * `Support::Readable`: the text is readable through TextPattern. The typo
//!   is selected with `TextRange.Select()`, overwritten, and read back.
//! * `Support::Blind`: the text is hidden from UIA, as in VS Code with
//!   accessibility support off, but each composition update is still reported.
//!   The typo is reached with arrow keys relative to the caret.
//! * `Support::Terminal`: a command line in a terminal, or any control put
//!   in `AppMode::Terminal`. The line before the cursor is read through UIA,
//!   the typo is replaced with arrow keys and Backspace, and the line is read
//!   back.
//!
//! The observer runs on its own thread because UIA client calls must not be
//! made from a thread that owns windows the client may end up inspecting.
//!
//! The crate is empty on targets other than Windows.

#[cfg(windows)]
mod event;
#[cfg(windows)]
mod ime;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod keyboard;
#[cfg(windows)]
mod observer;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod settings;
#[cfg(windows)]
mod terminal;
#[cfg(windows)]
mod tracker;
#[cfg(windows)]
mod uia;

#[cfg(windows)]
pub use event::{CorrectionRecord, Event, ExcludeReason, Outcome, Support};
#[cfg(windows)]
pub use observer::{MAX_FIXES_PER_EDIT, Observer};
#[cfg(windows)]
pub use settings::{AppMode, Settings, Shared};
