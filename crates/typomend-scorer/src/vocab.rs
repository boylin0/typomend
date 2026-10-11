// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tokenizer for Chinese BERT vocabularies.
//!
//! Chinese BERT models split every CJK character into its own token, which
//! keeps a character position and a token position one to one; that is all
//! a scored position needs. Runs of ASCII letters and digits, as in the
//! English words of mixed text, are split into WordPiece tokens the way the
//! models were trained, such as `report` or `rep ##ort`, rather than one
//! letter per token. On the mixed test set, this lowers the share of correct
//! sentences the corrector changes by several points. Those characters are
//! never scored, so they map to no position.

use std::collections::HashMap;

use crate::Error;

/// Longest input the model accepts, including `[CLS]` and `[SEP]`.
const MAX_TOKENS: usize = 512;

/// Words longer than this become `[UNK]`, as in BERT's own tokenizer.
const MAX_WORD: usize = 100;

/// A BERT vocabulary with the ids of its special tokens.
#[derive(Debug)]
pub(crate) struct Vocab {
    /// Ids of single-character tokens.
    chars: HashMap<char, u32>,
    /// Multi-character tokens, including `##` continuations.
    pieces: HashMap<String, u32>,
    /// Id of `[MASK]`.
    pub(crate) mask: u32,
    cls: u32,
    sep: u32,
    unk: u32,
}

/// Token ids for a piece of text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Encoded {
    /// Token ids, wrapped in `[CLS]` and `[SEP]`.
    pub(crate) ids: Vec<u32>,
    /// Token index of each character, `None` for whitespace, for letters
    /// and digits inside a word, and for characters cut off because the
    /// text was too long.
    pub(crate) token_of: Vec<Option<usize>>,
}

impl Vocab {
    /// Parses a BERT `vocab.txt`, where the token on line `n` has id `n`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Vocab`] when the file has too many tokens or lacks
    /// `[CLS]`, `[SEP]`, `[MASK]` or `[UNK]`.
    pub(crate) fn parse(text: &str) -> Result<Self, Error> {
        let mut chars = HashMap::new();
        let mut pieces = HashMap::new();
        for (id, token) in text.lines().enumerate() {
            let id = u32::try_from(id).map_err(|_| Error::Vocab("too many tokens".into()))?;
            let mut it = token.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => {
                    chars.entry(c).or_insert(id);
                }
                _ => {
                    pieces.entry(token.to_owned()).or_insert(id);
                }
            }
        }
        let get = |name: &str| {
            pieces
                .get(name)
                .copied()
                .ok_or_else(|| Error::Vocab(format!("missing {name}")))
        };
        Ok(Self {
            cls: get("[CLS]")?,
            sep: get("[SEP]")?,
            mask: get("[MASK]")?,
            unk: get("[UNK]")?,
            chars,
            pieces,
        })
    }

    /// Returns the id of `c`, or `None` when the vocabulary lacks it.
    pub(crate) fn id(&self, c: char) -> Option<u32> {
        if let Some(&id) = self.chars.get(&c) {
            return Some(id);
        }
        // The vocabulary is lowercase; only single-char lowercase forms can
        // be looked up, which covers every letter that matters here.
        let mut lower = c.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(l), None) => self.chars.get(&l).copied(),
            _ => None,
        }
    }

    /// Returns the id of a WordPiece token, single characters included.
    fn piece(&self, token: &str) -> Option<u32> {
        let mut it = token.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => self.chars.get(&c).copied(),
            _ => self.pieces.get(token).copied(),
        }
    }

    /// Splits a lowercase ASCII word into the longest WordPiece tokens,
    /// left to right, or a single `[UNK]` when some part has no token.
    fn wordpiece(&self, word: &str) -> Vec<u32> {
        if word.len() > MAX_WORD {
            return vec![self.unk];
        }
        let mut ids = Vec::new();
        let mut start = 0;
        while start < word.len() {
            let found = (start + 1..=word.len()).rev().find_map(|end| {
                let sub = &word[start..end];
                let id = if start == 0 {
                    self.piece(sub)
                } else {
                    self.piece(&format!("##{sub}"))
                };
                id.map(|id| (id, end))
            });
            let Some((id, end)) = found else {
                return vec![self.unk];
            };
            ids.push(id);
            start = end;
        }
        ids
    }

    /// Encodes `text`, keeping only its end when it is longer than the model
    /// accepts, since the end is where the user is typing.
    pub(crate) fn encode(&self, text: &str) -> Encoded {
        let chars: Vec<char> = text.chars().collect();
        let mut ids = Vec::new();
        let mut token_of = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_alphanumeric() {
                let end = chars[i..]
                    .iter()
                    .position(|c| !c.is_ascii_alphanumeric())
                    .map_or(chars.len(), |n| i + n);
                let word: String = chars[i..end].iter().map(char::to_ascii_lowercase).collect();
                ids.extend(self.wordpiece(&word));
                token_of.extend(std::iter::repeat_n(None, end - i));
                i = end;
                continue;
            }
            if c.is_whitespace() {
                token_of.push(None);
            } else {
                token_of.push(Some(ids.len()));
                ids.push(self.id(c).unwrap_or(self.unk));
            }
            i += 1;
        }

        let skip = ids.len().saturating_sub(MAX_TOKENS - 2);
        let mut window = Vec::with_capacity(ids.len() - skip + 2);
        window.push(self.cls);
        window.extend_from_slice(&ids[skip..]);
        window.push(self.sep);
        for t in &mut token_of {
            // Shift by one for [CLS].
            *t = t.and_then(|i| i.checked_sub(skip)).map(|i| i + 1);
        }
        Encoded {
            ids: window,
            token_of,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ids: [PAD] 0, [UNK] 1, [CLS] 2, [SEP] 3, [MASK] 4, 因 5, 為 6, 問 7,
    // a 8, ##s 9, re 10, ##port 11, report 12, 1 13, ##0 14, ##p 15.
    const VOCAB: &str =
        "[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\n因\n為\n問\na\n##s\nre\n##port\nreport\n1\n##0\n##p\n";

    fn vocab() -> Vocab {
        Vocab::parse(VOCAB).unwrap()
    }

    #[test]
    fn parses_special_tokens() {
        let v = vocab();
        assert_eq!((v.cls, v.sep, v.mask), (2, 3, 4));
        assert_eq!(v.id('為'), Some(6));
        assert_eq!(v.id('A'), Some(8));
        assert_eq!(v.id('我'), None);
    }

    #[test]
    fn skips_whitespace() {
        let e = vocab().encode("因 問我");
        assert_eq!(e.ids, vec![2, 5, 7, 1, 3]);
        assert_eq!(e.token_of, vec![Some(1), None, Some(2), Some(3)]);
    }

    #[test]
    fn splits_words_into_wordpieces() {
        let v = vocab();
        assert_eq!(v.wordpiece("report"), vec![12]);
        assert_eq!(v.wordpiece("as"), vec![8, 9]);
        assert_eq!(v.wordpiece("10"), vec![13, 14]);
        assert_eq!(v.wordpiece("rep"), vec![10, 15]);
        assert_eq!(v.wordpiece("rp"), vec![1]);
        assert_eq!(v.wordpiece("xyz"), vec![1]);
    }

    #[test]
    fn keeps_chinese_positions_around_words() {
        // 因 Report 為 10 問: words take no position, Chinese keeps its own.
        let e = vocab().encode("因Report為10問");
        assert_eq!(e.ids, vec![2, 5, 12, 6, 13, 14, 7, 3]);
        assert_eq!(e.token_of[0], Some(1));
        assert!(e.token_of[1..7].iter().all(Option::is_none));
        assert_eq!(e.token_of[7], Some(3));
        assert_eq!(e.token_of[10], Some(6));
    }

    #[test]
    fn keeps_the_end_of_long_text() {
        let text: String = std::iter::repeat_n('因', 2000).collect();
        let e = vocab().encode(&text);
        assert_eq!(e.ids.len(), MAX_TOKENS);
        assert_eq!(e.token_of[1999], Some(MAX_TOKENS - 2));
        assert_eq!(e.token_of[2000 - (MAX_TOKENS - 2)], Some(1));
        assert_eq!(e.token_of[0], None);
    }
}
