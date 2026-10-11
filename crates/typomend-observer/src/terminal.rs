// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Correcting command lines.
//!
//! Windows Terminal and the console host expose the screen buffer through
//! TextPattern, and with nothing selected the selection is an empty range at
//! the shell's cursor. An open IME composition is drawn over the buffer, not
//! written into it, so whatever appears before the cursor has already been
//! committed. Selecting text, though, only starts a mark mode, so the typo is
//! replaced the way the user would do it: Left to reach it, Backspace to
//! delete it, the correction typed, then Right back to where the cursor was.
//! Line editors such as PSReadLine, cmd and readline move by character, which
//! is why the keys are counted in characters rather than UTF-16 units.

use std::time::Instant;

use typomend_engine::{Correction, Corrector};
use windows::Win32::UI::Accessibility::IUIAutomationTextPattern;

use crate::event::{CorrectionRecord, Outcome};
use crate::observer::MAX_FIXES_PER_EDIT;
use crate::settings::Settings;
use crate::{input, keyboard, uia};

/// The most screen rows one wrapped command line is read from.
const MAX_ROWS: usize = 8;

/// What the observer remembers about the focused command line.
#[derive(Debug)]
pub(crate) struct Line {
    /// The line as last read.
    seen: String,
    /// The line as it was the last time it stopped changing. Only text added
    /// after it counts as just typed.
    settled: String,
    /// When the line last settled.
    settled_at: Instant,
    /// Corrections made since the line last settled.
    fixes: u8,
}

impl Line {
    /// Reads the command line up to the cursor and takes it as settled, so
    /// only text typed from now on is corrected.
    pub fn read(pattern: &IUIAutomationTextPattern) -> Self {
        let text = read(pattern).unwrap_or_default();
        Self {
            seen: text.clone(),
            settled: text,
            settled_at: Instant::now(),
            fixes: 0,
        }
    }

    /// Takes `text` as the settled line, with nothing typed since.
    fn settle(&mut self, text: String) {
        self.seen.clone_from(&text);
        self.settled = text;
        self.settled_at = Instant::now();
        self.fixes = 0;
    }

    /// Records a correction of the typo starting at byte `start`, after
    /// which the line reads `text`.
    fn corrected(&mut self, text: String, start: usize, verified: bool) {
        self.fixes += 1;
        if !verified || self.fixes >= MAX_FIXES_PER_EDIT {
            self.settle(text);
            return;
        }
        // The rest of what was just typed may hold another typo, so it stays
        // unsettled. The settled part must stay a prefix of the line, which
        // it is not once a typo that began inside it is corrected, as with 因
        // typed before and 該 typed after the line settled.
        self.seen.clone_from(&text);
        self.settled.truncate(start.min(self.settled.len()));
    }
}

/// Reads the command line up to the cursor, or returns `None` when the
/// terminal shows no cursor or the read fails.
fn read(pattern: &IUIAutomationTextPattern) -> Option<String> {
    uia::line_before_caret(pattern, MAX_ROWS).ok().flatten()
}

/// Returns the correction for text typed since the line last settled, if any.
///
/// The line must have grown at its end. Anything else, such as a new prompt
/// after Enter, Backspace, or a command recalled from history, starts over,
/// and a typo the user has already seen and left alone is never touched.
///
/// `within` is the number of characters before the end of `line` that a typo
/// may start in, as passed to [`Corrector::last_correction`].
pub(crate) fn fresh_correction(
    settled: &str,
    line: &str,
    corrector: &dyn Corrector,
    within: usize,
) -> Option<Correction> {
    if line.len() <= settled.len() || !line.starts_with(settled) {
        return None;
    }
    corrector
        .last_correction(line, within)
        .filter(|fix| fix.end() > settled.len())
}

/// Moves the correction of a command line forward by one observer tick.
/// Returns the correction attempted on this tick, if any.
pub(crate) fn tick(
    line: &mut Line,
    pattern: &IUIAutomationTextPattern,
    process: &str,
    settings: &Settings,
    corrector: &dyn Corrector,
) -> Option<CorrectionRecord> {
    let current = read(pattern)?;
    if current != line.seen {
        // Wait for one quiet interval so a burst of committed characters is
        // treated as a single edit.
        line.seen = current;
        return None;
    }
    if current == line.settled || input::idle_ms() < settings.idle_ms {
        return None;
    }
    // Programs print to the line on their own too. Only a change the user
    // made by typing is a candidate.
    let by_user = keyboard::last_key().is_some_and(|k| k > line.settled_at);
    let fix = by_user
        .then(|| fresh_correction(&line.settled, &current, corrector, settings.caret_window))
        .flatten();
    let Some(fix) = fix else {
        line.settle(current);
        return None;
    };

    // Each step is read back before the next is sent. A program that falls
    // behind a burst of keys then never gets keys meant for a cursor position
    // it has not reached yet, and a step that goes wrong stops the rest. Keys
    // can also be lost: Notepad with Microsoft Bopomofo sometimes drops one
    // right after a commit. The cursor is therefore checked before anything
    // is deleted, since Backspace from the wrong place destroys text.
    let after = current[fix.end()..].chars().count();
    let head = &current[..fix.start];
    let corrected = format!("{head}{}", fix.to);
    let expected = format!("{corrected}{}", &current[fix.end()..]);
    let reaches =
        |text: &str| crate::observer::wait_until(|| read(pattern).as_deref() == Some(text));
    let reached = input::tap_key(input::KEY_LEFT, after) && reaches(&current[..fix.end()]);
    let deleted =
        reached && input::tap_key(input::KEY_BACK, fix.from.chars().count()) && reaches(head);
    let typed = deleted && input::type_text(&fix.to) && reaches(&corrected);
    let verified = typed && input::tap_key(input::KEY_RIGHT, after) && reaches(&expected);
    if !verified {
        tracing::debug!(reached, deleted, typed, "terminal correction stopped");
    }
    line.corrected(read(pattern).unwrap_or(current), fix.start, verified);

    Some(CorrectionRecord {
        process: process.to_owned(),
        from: fix.from,
        to: fix.to,
        outcome: if verified {
            Outcome::Verified
        } else {
            Outcome::Failed
        },
    })
}

#[cfg(test)]
mod tests {
    use typomend_engine::{Rule, RuleCorrector};

    use super::*;

    fn corrector() -> RuleCorrector {
        RuleCorrector::new(vec![Rule::new("因該", "應該")])
    }

    const PROMPT: &str = "PS C:\\> ";

    #[test]
    fn corrects_text_committed_at_the_end() {
        let fix = fresh_correction(PROMPT, &format!("{PROMPT}因該"), &corrector(), 32);
        assert_eq!(fix.map(|f| f.to), Some("應該".to_owned()));
    }

    #[test]
    fn corrects_a_typo_completed_by_the_new_text() {
        let settled = format!("{PROMPT}echo 因");
        let fix = fresh_correction(&settled, &format!("{settled}該可以"), &corrector(), 32);
        assert_eq!(fix.map(|f| f.start), Some(settled.len() - "因".len()));
    }

    #[test]
    fn leaves_a_typo_typed_earlier_alone() {
        let settled = format!("{PROMPT}因該");
        assert_eq!(
            fresh_correction(&settled, &format!("{settled} 可以"), &corrector(), 32),
            None
        );
    }

    #[test]
    fn ignores_a_line_that_did_not_grow_at_its_end() {
        let c = corrector();
        // A new prompt after the command ran.
        assert_eq!(fresh_correction("PS C:\\> echo", PROMPT, &c, 32), None);
        // A command recalled from history replaces the line.
        assert_eq!(
            fresh_correction("PS C:\\> ls", "PS C:\\> echo 因該", &c, 32),
            None
        );
        // Nothing changed.
        let line = format!("{PROMPT}因該");
        assert_eq!(fresh_correction(&line, &line, &c, 32), None);
    }

    fn line(settled: &str) -> Line {
        Line {
            seen: settled.to_owned(),
            settled: settled.to_owned(),
            settled_at: Instant::now(),
            fixes: 0,
        }
    }

    #[test]
    fn corrects_every_typo_in_one_commit() {
        let c = RuleCorrector::new(vec![
            Rule::new("斗府", "豆腐"),
            Rule::new("茶原因", "查原因"),
        ]);
        let mut l = line(PROMPT);
        let mut text = format!("{PROMPT}臭斗府要茶原因");
        while let Some(fix) = fresh_correction(&l.settled, &text, &c, 32) {
            text.replace_range(fix.start..fix.end(), &fix.to);
            l.corrected(text.clone(), fix.start, true);
        }
        assert_eq!(text, format!("{PROMPT}臭豆腐要查原因"));
        assert_eq!(l.fixes, 2);
    }

    #[test]
    fn keeps_the_settled_text_a_prefix_after_a_correction() {
        let mut l = line(&format!("{PROMPT}echo 因"));
        let text = format!("{PROMPT}echo 應該可以");
        l.corrected(text.clone(), format!("{PROMPT}echo ").len(), true);
        assert!(text.starts_with(&l.settled));
        assert_eq!(l.settled, format!("{PROMPT}echo "));
    }

    #[test]
    fn settles_after_a_failed_correction_or_too_many() {
        let mut l = line(PROMPT);
        let text = format!("{PROMPT}應該");
        l.corrected(text.clone(), PROMPT.len(), false);
        assert_eq!(l.settled, text);

        let mut l = line(PROMPT);
        for _ in 0..MAX_FIXES_PER_EDIT {
            l.corrected(text.clone(), PROMPT.len(), true);
        }
        assert_eq!(l.settled, text);
        assert_eq!(l.fixes, 0);
    }

    #[test]
    fn ignores_a_typo_beyond_the_caret_window() {
        let settled = format!("{PROMPT}因該");
        let line = format!("{settled}{}", "字".repeat(40));
        assert_eq!(fresh_correction(PROMPT, &line, &corrector(), 32), None);
    }
}
