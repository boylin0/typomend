// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finds the positions where a homophone fits the context better than the
//! typed character.
//!
//! For each position, the typed character and its homophones are scored
//! together, and the result is the ratio of the best homophone's probability
//! to the typed character's. [`LmCorrector`](crate::LmCorrector) decides
//! what ratio is enough to correct.

use std::ops::Range;

use crate::{CandidateScorer, Context, Error, Homophones};

/// Probability floor for the typed character, so a character the model
/// considers impossible yields a large but finite ratio.
const FLOOR: f32 = 1e-9;

/// How positions are scored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Every position masked in turn. The most accurate and the slowest,
    /// with one sequence per position.
    Masked,
    /// One unmasked pass over the text for every position.
    OnePass,
    /// An unmasked pass to find suspects, then a masked pass over them.
    /// Costs about as much as [`Mode::OnePass`] on clean text.
    Hybrid {
        /// Ratio in the unmasked pass at which a position gets the masked
        /// pass.
        prefilter: f32,
    },
}

/// How strongly the best alternative beats the typed character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Assessment {
    /// Character position in the text.
    pub position: usize,
    /// The typed character.
    pub original: char,
    /// The most likely alternative.
    pub best: char,
    /// Probability of `best` divided by that of `original`. Above 1 the
    /// model prefers the alternative.
    pub ratio: f32,
    /// Probability of `best`.
    pub probability: f32,
}

/// Scores the homophones of typed characters against them; see the module
/// documentation.
#[derive(Debug)]
pub struct Detector {
    scorer: CandidateScorer,
    homophones: Homophones,
    mode: Mode,
}

impl Detector {
    /// Creates a detector that scores the candidates `homophones` proposes
    /// with `scorer`, in `mode`.
    pub fn new(scorer: CandidateScorer, homophones: Homophones, mode: Mode) -> Self {
        Self {
            scorer,
            homophones,
            mode,
        }
    }

    /// Returns the mode positions are scored in.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Assesses the characters of `text` within `positions` that have
    /// homophones. Positions out of range, on whitespace, or without any
    /// candidate are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error from the model.
    pub fn assess(&self, text: &str, positions: Range<usize>) -> Result<Vec<Assessment>, Error> {
        let chars: Vec<char> = text.chars().collect();
        let script = self.homophones.script_of(text);
        let queries: Vec<(usize, Vec<char>)> = positions
            .filter_map(|p| {
                let c = *chars.get(p)?;
                if c.is_whitespace() {
                    return None;
                }
                let others = self.homophones.candidates(c, script);
                if others.is_empty() {
                    return None;
                }
                Some((p, std::iter::once(c).chain(others).collect()))
            })
            .collect();
        if queries.is_empty() {
            return Ok(Vec::new());
        }

        match self.mode {
            Mode::Masked => self.run(text, &queries, Context::Masked),
            Mode::OnePass => self.run(text, &queries, Context::Unmasked),
            Mode::Hybrid { prefilter } => {
                let mut found = self.run(text, &queries, Context::Unmasked)?;
                let suspects: Vec<_> = queries
                    .iter()
                    .zip(&found)
                    .filter(|(_, a)| a.ratio >= prefilter)
                    .map(|(q, _)| q.clone())
                    .collect();
                // Replace each suspect's unmasked assessment with the masked
                // one, which is the more accurate of the two.
                if !suspects.is_empty() {
                    for confirmed in self.run(text, &suspects, Context::Masked)? {
                        if let Some(a) = found.iter_mut().find(|a| a.position == confirmed.position)
                        {
                            *a = confirmed;
                        }
                    }
                }
                Ok(found)
            }
        }
    }

    /// Scores `queries`, whose first candidate is the typed character, and
    /// returns the best alternative at each position.
    fn run(
        &self,
        text: &str,
        queries: &[(usize, Vec<char>)],
        context: Context,
    ) -> Result<Vec<Assessment>, Error> {
        let borrowed: Vec<(usize, &[char])> =
            queries.iter().map(|(p, c)| (*p, c.as_slice())).collect();
        let probs = self.scorer.score(text, &borrowed, context)?;
        Ok(queries
            .iter()
            .zip(probs)
            .map(|((position, candidates), probs)| {
                let (best, probability) = candidates[1..]
                    .iter()
                    .zip(&probs[1..])
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .map_or((candidates[0], 0.0), |(&c, &p)| (c, p));
                Assessment {
                    position: *position,
                    original: candidates[0],
                    best,
                    ratio: probability / probs[0].max(FLOOR),
                    probability,
                }
            })
            .collect())
    }
}
