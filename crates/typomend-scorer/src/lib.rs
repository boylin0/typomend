// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Masked language model correction for Typomend.
//!
//! The pipeline has three parts. [`Homophones`] proposes, for each typed
//! character, the characters a Bopomofo or pinyin user may have meant.
//! [`CandidateScorer`] asks a BERT-style masked language model how likely
//! each of them is in context. [`Detector`] combines the two over a range of
//! positions and reports how strongly the best alternative beats what was
//! typed, which [`LmCorrector`] thresholds to decide whether to correct.
//!
//! The model is the judge, not the search: it only ever compares the typed
//! character against a short candidate list, so a correction is always a
//! plausible slip of the input method. A typo like 因問 scores the typed 問
//! far below 為, and that gap is what the threshold acts on.
//!
//! A model lives in a directory holding `config.json`, `vocab.txt` and
//! `model.safetensors`; [`download`] fetches one from Hugging Face.
//! [`MODELS`] lists the ones that were evaluated.

pub mod bench;
mod corrector;
mod detector;
pub mod download;
mod homophones;
mod model;
mod models;
pub mod script;
mod vocab;

use std::fmt;
use std::path::Path;

use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::Config;

pub use corrector::{LmCorrector, Verdict};
pub use detector::{Assessment, Detector, Mode};
pub use homophones::{Homophones, Script};
pub use models::{MODELS, Measured, ModeKind, ModelInfo};

use model::MaskedLm;
use vocab::Vocab;

/// Most sequences scored in one forward pass. Larger batches gain little on
/// a CPU and hold more activations in memory.
const BATCH: usize = 16;

/// How the model sees the position being scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// The position is replaced by `[MASK]`, so the typed character cannot
    /// bias the result. Costs one sequence per position.
    Masked,
    /// The text is scored as typed. One sequence covers every position, but
    /// the model leans towards the character it can see.
    Unmasked,
}

/// A masked language model that scores candidate characters in context.
///
/// Create one with [`CandidateScorer::load`]. The scorer holds the weights
/// in memory for as long as it lives.
pub struct CandidateScorer {
    model: MaskedLm,
    vocab: Vocab,
    simplify: bool,
}

impl fmt::Debug for CandidateScorer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CandidateScorer")
            .field("simplify", &self.simplify)
            .finish_non_exhaustive()
    }
}

impl CandidateScorer {
    /// Loads a BERT masked language model from `dir` onto the CPU.
    ///
    /// With `simplify`, text and candidates are converted to Simplified
    /// characters before scoring; see [`script`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when a model file cannot be read,
    /// [`Error::Config`] when `config.json` is not a BERT configuration,
    /// [`Error::Vocab`] when `vocab.txt` lacks a special token, and
    /// [`Error::Model`] when the weights do not load.
    pub fn load(dir: impl AsRef<Path>, simplify: bool) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let mut config: serde_json::Value = serde_json::from_str(&read(&dir.join("config.json"))?)?;
        // Some checkpoints omit fields that transformers fills with defaults
        // but candle requires, such as `pad_token_id`.
        if let Some(fields) = config.as_object_mut() {
            fields.entry("pad_token_id").or_insert(0.into());
            fields.entry("initializer_range").or_insert(0.02.into());
        }
        let config: Config = serde_json::from_value(config)?;
        let vocab = Vocab::parse(&read(&dir.join("vocab.txt"))?)?;
        let weights = dir.join("model.safetensors");
        // SAFETY: a memory map is sound only while nobody truncates or
        // rewrites the file. Nothing does while the scorer lives: the file
        // is a finished model download that is only ever read, and callers
        // drop the scorer before they delete a model.
        let mut vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &Device::Cpu)? };
        // Checkpoints converted from the original TensorFlow BERT, such as
        // google-bert/bert-base-chinese, keep its gamma and beta names.
        if !vb.contains_tensor("bert.embeddings.LayerNorm.weight")
            && vb.contains_tensor("bert.embeddings.LayerNorm.gamma")
        {
            vb = vb.rename_f(|name| {
                name.replace("LayerNorm.weight", "LayerNorm.gamma")
                    .replace("LayerNorm.bias", "LayerNorm.beta")
            });
        }
        Ok(Self {
            model: MaskedLm::load(vb, &config)?,
            vocab,
            simplify,
        })
    }

    /// Scores each of `candidates` as the character at `position` of `text`,
    /// with that position masked. Scores come back in the order of
    /// `candidates`; include the typed character to compare against it.
    ///
    /// `position` counts characters, not bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Position`] when `position` is past the end of `text`
    /// or names whitespace, or an error from the model.
    pub fn score_candidates(
        &self,
        text: &str,
        position: usize,
        candidates: &[char],
    ) -> Result<Vec<Score>, Error> {
        let probs = self.score(text, &[(position, candidates)], Context::Masked)?;
        Ok(candidates
            .iter()
            .zip(probs.into_iter().flatten())
            .map(|(&candidate, probability)| Score {
                candidate,
                probability,
            })
            .collect())
    }

    /// Scores several positions of `text` at once. Each query is a character
    /// position and its candidates; the result holds one probability per
    /// candidate, over the whole vocabulary, in query order. A candidate the
    /// vocabulary lacks scores 0. `context` chooses whether the model sees
    /// the typed characters; see [`Context`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Position`] for a position past the end of `text`,
    /// on whitespace or cut off because the text is too long, or an error
    /// from the model.
    pub fn score(
        &self,
        text: &str,
        queries: &[(usize, &[char])],
        context: Context,
    ) -> Result<Vec<Vec<f32>>, Error> {
        let text: String = text.chars().map(|c| self.convert(c)).collect();
        let encoded = self.vocab.encode(&text);
        let tokens = queries
            .iter()
            .map(|&(position, _)| {
                encoded
                    .token_of
                    .get(position)
                    .copied()
                    .flatten()
                    .ok_or(Error::Position(position))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let dists = match context {
            Context::Unmasked => {
                let rows: Vec<_> = tokens.iter().map(|&t| (0, t)).collect();
                self.model.probabilities(&[encoded.ids], &rows)?
            }
            Context::Masked => {
                let mut dists = Vec::with_capacity(tokens.len());
                for chunk in tokens.chunks(BATCH) {
                    let batch: Vec<Vec<u32>> = chunk
                        .iter()
                        .map(|&t| {
                            let mut ids = encoded.ids.clone();
                            ids[t] = self.vocab.mask;
                            ids
                        })
                        .collect();
                    let rows: Vec<_> = chunk.iter().enumerate().map(|(i, &t)| (i, t)).collect();
                    dists.extend(self.model.probabilities(&batch, &rows)?);
                }
                dists
            }
        };

        Ok(queries
            .iter()
            .zip(dists)
            .map(|(&(_, candidates), dist)| {
                candidates
                    .iter()
                    .map(|&c| {
                        self.vocab
                            .id(self.convert(c))
                            .and_then(|id| dist.get(id as usize).copied())
                            .unwrap_or(0.0)
                    })
                    .collect()
            })
            .collect())
    }

    /// Returns `c` in the script the model scores text in, which is
    /// Simplified when the scorer was loaded with `simplify`.
    fn convert(&self, c: char) -> char {
        if self.simplify {
            script::to_simplified(c)
        } else {
            c
        }
    }
}

/// How likely a candidate is at the masked position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// The candidate character.
    pub candidate: char,
    /// Probability over the whole vocabulary, in `[0, 1]`. It is 0 for a
    /// candidate the vocabulary does not contain.
    pub probability: f32,
}

/// Reads the file at `path`, naming the path in the error.
fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::Io(path.display().to_string(), e))
}

/// Errors from loading, scoring with or downloading a model.
#[derive(Debug)]
pub enum Error {
    /// A file could not be read or written. Holds its path and the cause.
    Io(String, std::io::Error),
    /// `config.json` is not a valid BERT configuration.
    Config(serde_json::Error),
    /// `vocab.txt` has too many tokens or lacks a special token.
    Vocab(String),
    /// The weights could not be loaded or the forward pass failed.
    Model(candle_core::Error),
    /// No scorable character is at this character position: it is past the
    /// end of the text, whitespace, or cut off from text too long for the
    /// model.
    Position(usize),
    /// A download request failed or the server answered it wrongly.
    Http(String),
    /// A download was cancelled; it resumes when started again.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "reading {path}: {e}"),
            Self::Config(e) => write!(f, "invalid config.json: {e}"),
            Self::Vocab(msg) => write!(f, "invalid vocab.txt: {msg}"),
            Self::Model(e) => write!(f, "model: {e}"),
            Self::Position(p) => write!(f, "no scorable character at position {p}"),
            Self::Http(e) => write!(f, "{e}"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(_, e) => Some(e),
            Self::Config(e) => Some(e),
            Self::Model(e) => Some(e),
            Self::Vocab(_) | Self::Position(_) | Self::Http(_) | Self::Cancelled => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Config(e)
    }
}

impl From<candle_core::Error> for Error {
    fn from(e: candle_core::Error) -> Self {
        Self::Model(e)
    }
}
