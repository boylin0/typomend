// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text correction engine for Typomend.
//!
//! The engine is deliberately free of any platform code: it receives text the
//! user has just committed and answers which part of it should be replaced.
//! Detecting input and writing the replacement back is the job of the
//! observer, so another engine, such as the masked language model
//! in typomend-scorer, only has to implement [`Corrector`].
//!
//! [`RuleCorrector`] is the engine in this crate: a list of fixed
//! substitutions, read from a plain-text file with [`parse_rules`].

mod parse;
mod rules;

pub use parse::{ParseError, ParseErrorKind, parse_rules};
pub use rules::RuleCorrector;

/// A fixed text substitution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rule {
    /// Text to look for.
    pub from: String,
    /// Text to put in its place.
    pub to: String,
}

impl Rule {
    /// Creates a rule that replaces `from` with `to`.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// A single replacement found in a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    /// Byte offset of the text to replace.
    pub start: usize,
    /// Text being replaced; `text[start..start + from.len()] == from`.
    pub from: String,
    /// Replacement text.
    pub to: String,
}

impl Correction {
    /// Returns the byte offset just past the replaced text.
    pub fn end(&self) -> usize {
        self.start + self.from.len()
    }
}

/// Decides how freshly committed text should be corrected.
///
/// Correctors are shared between threads: the observer calls one from its
/// own thread, and a benchmark may call one from several at once.
pub trait Corrector: Send + Sync {
    /// Returns the correction that ends closest to the end of `text`, or
    /// `None` when the text needs no correction.
    ///
    /// Only corrections ending within `within` characters of the end of
    /// `text` are considered, which is where the user has just typed. An
    /// implementation may therefore look at the tail of `text` only, so the
    /// cost does not grow with the length of the document.
    ///
    /// Only one correction is returned because the caller edits a live
    /// document: after applying it, the caller re-reads the text and asks
    /// again if it wants more.
    fn last_correction(&self, text: &str, within: usize) -> Option<Correction>;
}
