// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Candidate generator: which characters a typed character may have been
//! meant as.
//!
//! Bopomofo and pinyin users pick the wrong character among those sharing a
//! reading, so candidates are the characters whose reading matches after
//! dropping the tone and folding the distinctions speakers commonly blur:
//! ㄓㄔㄕ against ㄗㄘㄙ and ㄣ against ㄥ, including ㄢ against ㄤ. Tones are
//! ignored because a missing or wrong tone key is a common slip and the
//! masked language model, not the reading, decides which candidate fits.
//!
//! Readings cover the common characters of both scripts: Big5 level 1 for
//! Traditional and the Table of General Standard Chinese Characters for
//! Simplified. Each character is tagged with the script it is common in, and
//! candidates only come from the script of the text, so 為 is never offered
//! as 为 or the other way round. Text that does not tell its script, such as
//! 我持到了, is taken to be in the script the user writes in; otherwise 遲,
//! which only Traditional Chinese uses, could never replace 持. A short list
//! of confusions the readings cannot explain, such as keys that sit next to
//! each other, adds the rest.

use std::collections::{BTreeSet, HashMap};

use crate::script::{is_simplified_form, to_simplified};

/// Readings table: a character, the script it is common in (`T`, `S` or
/// anything else for both) and its numbered pinyin readings, tab separated.
const READINGS: &str = include_str!("../data/readings.txt");

/// Confusion groups: lines of characters that are mistaken for one another.
const CONFUSIONS: &str = include_str!("../data/confusions.txt");

/// The script a piece of text is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// Traditional Chinese characters.
    Traditional,
    /// Simplified Chinese characters.
    Simplified,
    /// No character tells, as in 我今天去打球. Only characters common to both
    /// scripts are offered.
    Unknown,
}

/// Where a character is in common use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Use {
    /// Common in both scripts, so it fits text in either.
    Both,
    /// Common in Traditional text only.
    Traditional,
    /// Common in Simplified text only.
    Simplified,
}

impl Use {
    /// Returns whether a character used this way may be offered in text
    /// written in `script`. Text of an [`Script::Unknown`] script takes only
    /// the characters common to both.
    fn fits(self, script: Script) -> bool {
        matches!(
            (self, script),
            (Self::Both, _)
                | (Self::Traditional, Script::Traditional)
                | (Self::Simplified, Script::Simplified)
        )
    }
}

/// Proposes the characters a typed character may have been meant as.
///
/// Create one with [`Homophones::builtin`]. See the module documentation for
/// how candidates are chosen.
#[derive(Debug)]
pub struct Homophones {
    /// Fuzzy reading keys of each character.
    keys: HashMap<char, Vec<String>>,
    /// Characters sharing each fuzzy reading key.
    by_key: HashMap<String, Vec<char>>,
    /// Script each character is common in.
    uses: HashMap<char, Use>,
    /// Characters confused with each character for reasons other than the
    /// reading.
    extra: HashMap<char, Vec<char>>,
    /// What [`Homophones::script_of`] returns for text that does not tell.
    fallback: Script,
}

impl Homophones {
    /// Returns the generator built from the bundled tables.
    pub fn builtin() -> Self {
        let mut keys: HashMap<char, Vec<String>> = HashMap::new();
        let mut by_key: HashMap<String, Vec<char>> = HashMap::new();
        let mut uses = HashMap::new();
        for line in data_lines(READINGS) {
            let mut parts = line.split('\t');
            let (Some(c), Some(script), Some(readings)) =
                (parts.next().and_then(single), parts.next(), parts.next())
            else {
                continue;
            };
            let used = match script {
                "T" => Use::Traditional,
                "S" => Use::Simplified,
                _ => Use::Both,
            };
            uses.insert(c, used);
            let fuzzy: BTreeSet<String> = readings.split(' ').map(fuzzy_key).collect();
            for key in fuzzy {
                by_key.entry(key.clone()).or_default().push(c);
                keys.entry(c).or_default().push(key);
            }
        }

        let mut extra: HashMap<char, Vec<char>> = HashMap::new();
        for line in data_lines(CONFUSIONS) {
            let group: Vec<char> = line.split_whitespace().filter_map(single).collect();
            for &a in &group {
                extra
                    .entry(a)
                    .or_default()
                    .extend(group.iter().filter(|&&b| b != a));
            }
        }
        Self {
            keys,
            by_key,
            uses,
            extra,
            fallback: Script::Unknown,
        }
    }

    /// Takes text that does not tell its script to be in `script`, the one
    /// the user writes in, instead of [`Script::Unknown`].
    #[must_use]
    pub fn assuming(mut self, script: Script) -> Self {
        self.fallback = script;
        self
    }

    /// Guesses the script of `text` from the characters only one script
    /// uses, taking the majority. A tie, including text where every
    /// character is common to both, gives the script set with
    /// [`Homophones::assuming`].
    pub fn script_of(&self, text: &str) -> Script {
        let (mut traditional, mut simplified) = (0, 0);
        for c in text.chars() {
            match self.uses.get(&c) {
                Some(Use::Traditional) => traditional += 1,
                Some(Use::Simplified) => simplified += 1,
                _ => {}
            }
        }
        match traditional.cmp(&simplified) {
            std::cmp::Ordering::Greater => Script::Traditional,
            std::cmp::Ordering::Less => Script::Simplified,
            std::cmp::Ordering::Equal => self.fallback,
        }
    }

    /// Returns the characters `c` may have been meant as in text written in
    /// `script`, without `c` itself. The list is empty for characters
    /// without a known reading, such as punctuation.
    ///
    /// A character is left out when it is `c` written in the other script,
    /// as 于 is for 於, or the usual way to write another candidate in the
    /// other script: for 摸 in Traditional text, 麼 is offered but 么 is not.
    pub fn candidates(&self, c: char, script: Script) -> Vec<char> {
        let mut out: Vec<char> = self
            .keys
            .get(&c)
            .into_iter()
            .flatten()
            .filter_map(|k| self.by_key.get(k))
            .flatten()
            .chain(self.extra.get(&c).into_iter().flatten())
            .copied()
            .filter(|&o| {
                o != c
                    && self.uses.get(&o).is_none_or(|u| u.fits(script))
                    && !other_form(c, o, script, is_simplified_form)
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        // Drop a candidate that is just the usual Simplified form of another
        // candidate, so 麼 is offered for 摸 but not 么. Only the usual form
        // counts: 具 is also a Simplified form of 俱, and dropping it would
        // miss a typo for 具 itself.
        let usual = |s: char, t: char| t != s && to_simplified(t) == s;
        let all = out.clone();
        out.retain(|&o| !all.iter().any(|&p| other_form(p, o, script, usual)));
        out
    }
}

/// Returns whether `o` is `c` written in the other script, such as 于 for
/// 於 in Traditional text.
///
/// `simplified(s, t)` tells whether `s` counts as a Simplified form of `t`.
/// Both characters may be common in either script, but the user wrote `c` in
/// the script of the text, so `o` is not what they meant.
fn other_form(c: char, o: char, script: Script, simplified: impl Fn(char, char) -> bool) -> bool {
    match script {
        Script::Traditional => simplified(o, c),
        Script::Simplified => simplified(c, o),
        Script::Unknown => false,
    }
}

/// Returns the lines of a data file that are neither blank nor comments.
fn data_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// Returns the character of `s` when it holds exactly one.
fn single(s: &str) -> Option<char> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// Reduces a numbered pinyin syllable such as `zheng4` to the key shared by
/// the readings it is confused with, here `zen`.
fn fuzzy_key(syllable: &str) -> String {
    let s = syllable.trim_end_matches(|c: char| c.is_ascii_digit());
    let s = ["zh", "ch", "sh"]
        .iter()
        .find_map(|p| s.strip_prefix(p).map(|rest| format!("{}{rest}", &p[..1])))
        .unwrap_or_else(|| s.to_owned());
    match s.strip_suffix('g') {
        Some(base) if base.ends_with('n') && !base.ends_with("on") => base.to_owned(),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_retroflex_and_nasal_finals() {
        assert_eq!(fuzzy_key("zheng4"), "zen");
        assert_eq!(fuzzy_key("ying1"), "yin");
        assert_eq!(fuzzy_key("chuang2"), "cuan");
        assert_eq!(fuzzy_key("dong4"), "dong");
        assert_eq!(fuzzy_key("me5"), "me");
    }

    #[test]
    fn suggests_homophones_and_confusions() {
        let h = Homophones::builtin();
        let t = Script::Traditional;
        let has = |c, o| h.candidates(c, t).contains(&o);
        assert!(has('投', '頭'));
        assert!(has('因', '應'));
        assert!(has('在', '再'));
        assert!(has('問', '為'));
        assert!(has('為', '問'));
        assert!(has('求', '球'));
        assert!(!has('投', '投'));
        assert!(h.candidates('，', t).is_empty());
    }

    #[test]
    fn keeps_candidates_in_the_script_of_the_text() {
        let h = Homophones::builtin();
        let t = h.candidates('投', Script::Traditional);
        let s = h.candidates('投', Script::Simplified);
        let u = h.candidates('投', Script::Unknown);
        assert!(t.contains(&'頭') && !t.contains(&'头'));
        assert!(s.contains(&'头') && !s.contains(&'頭'));
        assert!(!u.contains(&'頭') && !u.contains(&'头'));
        assert!(h.candidates('问', Script::Simplified).contains(&'为'));
    }

    #[test]
    fn never_offers_a_character_in_the_other_script() {
        let h = Homophones::builtin();
        let t = Script::Traditional;
        assert!(!h.candidates('於', t).contains(&'于'));
        assert!(!h.candidates('瞭', t).contains(&'了'));
        assert!(h.candidates('於', t).contains(&'魚'));
        assert!(!h.candidates('于', Script::Simplified).contains(&'於'));
        let what = h.candidates('摸', t);
        assert!(what.contains(&'麼') && !what.contains(&'么'));
        assert!(h.candidates('摸', Script::Simplified).contains(&'么'));
        assert!(h.candidates('句', t).contains(&'具'));
    }

    #[test]
    fn tells_the_script_of_text() {
        let h = Homophones::builtin();
        assert_eq!(h.script_of("我今天去打籃求"), Script::Traditional);
        assert_eq!(h.script_of("我今天去打篮求"), Script::Simplified);
        assert_eq!(h.script_of("我今天去打球"), Script::Unknown);
    }

    #[test]
    fn assumes_the_users_script_when_the_text_does_not_tell() {
        let h = Homophones::builtin().assuming(Script::Traditional);
        assert_eq!(h.script_of("我持到了"), Script::Traditional);
        assert_eq!(h.script_of("我迟到了"), Script::Simplified);
        let script = h.script_of("你在做什摸");
        assert!(h.candidates('持', h.script_of("我持到了")).contains(&'遲'));
        assert!(h.candidates('摸', script).contains(&'麼'));
    }
}
