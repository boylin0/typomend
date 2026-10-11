// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What the observer reports to the rest of the program.

/// How the observer treats the focused control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Text is readable; corrections are verified by reading them back.
    Readable,
    /// Text is hidden but composition updates are reported; corrections are
    /// typed relative to the caret and cannot be verified.
    Blind,
    /// A command line: the line before the cursor is readable, but selecting
    /// text does not move the cursor, so corrections are typed relative to it
    /// and then read back.
    Terminal,
    /// The control is deliberately left alone.
    Excluded(ExcludeReason),
    /// The control exposes nothing the observer can work with.
    Unsupported,
}

/// Why a control is left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludeReason {
    /// A password field.
    Password,
    /// The user disabled corrections for this program.
    DisabledByUser,
    /// A terminal in [`AppMode::Standard`](crate::AppMode::Standard), which
    /// corrects by selecting text. In a terminal that only starts a mark mode
    /// and leaves the input cursor where it is.
    Terminal,
    /// The program runs elevated and Windows blocks input sent to it.
    Elevated,
    /// Blind mode is switched off in the settings.
    BlindModeOff,
}

/// Result of one correction attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The replacement was read back from the control.
    Verified,
    /// The replacement was sent but the control cannot be read back.
    Unverified,
    /// The replacement did not show up, or was not attempted because the
    /// control did not behave as expected.
    Failed,
    /// The user kept typing after the commit, so the caret position was no
    /// longer known and nothing was sent.
    Skipped,
    /// The control answered too slowly to change it safely, as a browser
    /// does with a very long document. A key the user pressed between
    /// selecting the typo and replacing it would overwrite the selection, so
    /// nothing was sent.
    TooSlow,
}

/// A correction attempt, as shown in the history.
#[derive(Debug, Clone)]
pub struct CorrectionRecord {
    /// Executable name without extension, such as `chrome`.
    pub process: String,
    /// The typo.
    pub from: String,
    /// What it was to be replaced with.
    pub to: String,
    /// How the attempt ended.
    pub outcome: Outcome,
}

/// A notification from the observer thread.
#[derive(Debug, Clone)]
pub enum Event {
    /// Focus moved to a control in `process`.
    Focus {
        /// Executable name without extension, such as `chrome`.
        process: String,
        /// How the observer treats the control.
        support: Support,
    },
    /// A correction was attempted.
    Correction(CorrectionRecord),
}
