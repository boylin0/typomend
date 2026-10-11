// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Turns the detector's assessments into corrections.
//!
//! A character is replaced when its best homophone is at least a threshold
//! times likelier than what was typed. The last characters typed have the
//! least context, so they must beat a stricter threshold until more text
//! follows, or until the model is far surer of them once the text is taken
//! to end there.

use typomend_engine::{Correction, Corrector};

use crate::{Assessment, Detector, Error, Mode};

/// Characters of context read before the checked ones. The test sentences
/// are about this long, and every extra character makes each masked pass
/// slower: with 32, a base model takes about 180 ms per call instead of
/// 130 ms.
const CONTEXT: usize = 16;

/// Most characters checked per call. Each costs a model pass in masked
/// mode, and older characters were checked on earlier calls as they were
/// typed.
const WINDOW: usize = 8;

/// Characters a position must be followed by to be checked at the normal
/// threshold.
///
/// The model judges a character by both sides. On the test set, with
/// nothing after the typo, the best model fixes 56% of typos instead of 91%,
/// and false positives rise with it; three characters after it recover most
/// of the difference. The caller checks again as the user keeps typing, so
/// the normal threshold applies once a character or two follow.
const MIN_RIGHT: usize = 2;

/// How much more sure the model must be about one of the last
/// [`MIN_RIGHT`] characters. They are still checked, since the user may
/// stop typing right after a typo, as in 我今天去打籃求; there the model
/// prefers 球 by a ratio of about 300,000, far above any threshold.
const TAIL_FACTOR: f32 = 10.0;

/// How much more sure than the normal threshold the model must be about one
/// of the last characters when the text is taken to end right after it, for
/// a character that passes the normal threshold but not [`TAIL_FACTOR`].
///
/// With nothing after it, a typo at the end often falls short: in
/// 今天要去吃臭豆府 the model prefers 腐 only 9,000 times, but 5,000,000
/// times in 今天要去吃臭豆府。. Scoring every character as if a 。 followed
/// changes 4% of the correct test sentences, since most text goes on; asking
/// this much of it changes none and fixes one or two more typos per set.
///
/// [`Mode::OnePass`] is left out. Its ratios come from unmasked text and its
/// threshold is far lower, and there the check changes 2 of the 40 correct
/// mixed Chinese and English sentences.
const ENDED_FACTOR: f32 = 100.0;

/// Character appended to take the text as ended; see [`ENDED_FACTOR`].
const ENDED: char = '。';

/// Characters that end a clause, after which nothing more is coming that
/// would help, so the last characters get the normal threshold.
const CLAUSE_END: &[char] = &[
    '。', '！', '？', '，', '、', '；', '：', '…', '!', '?', ',', ';',
];

/// A [`Corrector`] that replaces a recently typed character when the
/// masked language model prefers one of its homophones by at least a threshold.
///
/// Each call checks up to the last eight characters within the caret
/// window, with sixteen more before them as context, and returns only the
/// correction the model is surest of. The caller asks again after applying
/// it.
#[derive(Debug)]
pub struct LmCorrector {
    detector: Detector,
    threshold: f32,
}

/// How [`LmCorrector`] judges one character; see [`LmCorrector::explain`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    /// How the model judges the character.
    pub assessment: Assessment,
    /// Ratio the best alternative must reach to replace the character.
    pub required: f32,
    /// For one of the last characters that reaches the normal threshold but
    /// not `required`, how it is judged with the text taken to end after it.
    pub ended: Option<Assessment>,
    /// Whether the character would be replaced, were it the only one to
    /// qualify.
    pub passes: bool,
}

impl LmCorrector {
    /// Creates a corrector that replaces a character when `detector` finds a
    /// homophone at least `threshold` times likelier than it.
    pub fn new(detector: Detector, threshold: f32) -> Self {
        Self {
            detector,
            threshold,
        }
    }

    /// Assesses every character of `text` that has homophones, with the
    /// threshold it must pass, to show why text was or was not corrected.
    ///
    /// Unlike [`Corrector::last_correction`], which only looks at the last
    /// few characters typed, every character is assessed, and all of `text`
    /// is the context.
    ///
    /// # Errors
    ///
    /// Returns an error from the model.
    pub fn explain(&self, text: &str) -> Result<Vec<Verdict>, Error> {
        let len = text.chars().count();
        self.judge(text, self.detector.assess(text, 0..len)?)
    }

    /// Decides which of the characters of `text` assessed in `found` would
    /// be replaced.
    fn judge(&self, text: &str, found: Vec<Assessment>) -> Result<Vec<Verdict>, Error> {
        let settled = settled(text, text.chars().count());
        found
            .into_iter()
            .map(|assessment| {
                let p = assessment.position;
                let required = self.required(p, settled);
                let mut verdict = Verdict {
                    assessment,
                    required,
                    ended: None,
                    passes: assessment.ratio >= required,
                };
                let second_look = !verdict.passes
                    && p >= settled
                    && assessment.ratio >= self.threshold
                    && self.detector.mode() != Mode::OnePass;
                if second_look {
                    let ended = self.detector.assess(&format!("{text}{ENDED}"), p..p + 1)?;
                    verdict.ended = ended.first().copied();
                    verdict.passes = verdict.ended.is_some_and(|e| {
                        e.best == assessment.best && e.ratio >= self.threshold * ENDED_FACTOR
                    });
                }
                Ok(verdict)
            })
            .collect()
    }

    /// Returns the ratio needed at `position` when the positions from
    /// `settled` on are among the last typed.
    fn required(&self, position: usize, settled: usize) -> f32 {
        if position < settled {
            self.threshold
        } else {
            self.threshold * TAIL_FACTOR
        }
    }
}

/// Returns how many of the `len` characters of `text` are followed by enough
/// text to be judged at the normal threshold.
fn settled(text: &str, len: usize) -> usize {
    if text.trim_end().ends_with(CLAUSE_END) {
        len
    } else {
        len.saturating_sub(MIN_RIGHT)
    }
}

impl Corrector for LmCorrector {
    fn last_correction(&self, text: &str, within: usize) -> Option<Correction> {
        let window = within.min(WINDOW);
        let total = text.chars().count();
        let skip = total.saturating_sub(window + CONTEXT);
        let start = text.char_indices().nth(skip).map_or(text.len(), |(i, _)| i);
        let tail = &text[start..];
        let len = total - skip;

        let found = self
            .detector
            .assess(tail, len.saturating_sub(window)..len)
            .ok()?;
        // Only one correction is returned per call, so take the one the
        // model is surest of; the caller asks again after applying it.
        let best = self
            .judge(tail, found)
            .ok()?
            .into_iter()
            .filter(|v| v.passes)
            .map(|v| v.assessment)
            .max_by(|a, b| a.ratio.total_cmp(&b.ratio))?;
        let offset = tail.char_indices().nth(best.position)?.0;
        Some(Correction {
            start: start + offset,
            from: best.original.to_string(),
            to: best.best.to_string(),
        })
    }
}
