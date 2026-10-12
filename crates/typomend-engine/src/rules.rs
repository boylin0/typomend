// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The rule list engine.
//!
//! All rules are compiled into one Aho-Corasick automaton when the list is
//! loaded. A lookup then walks the searched text once, whatever the number of
//! rules, so a list of hundreds of thousands of entries answers as fast as a
//! list of one. Only the tail of the text that can still hold a match near
//! the caret is searched.

use std::collections::HashMap;

use aho_corasick::{AhoCorasick, MatchKind};

use crate::{Correction, Corrector, Rule, parse_rules};

/// Misspellings everyone makes, mostly of idioms, such as 莫名奇妙 for
/// 莫名其妙.
const COMMON_TYPOS: &str = include_str!("../data/common-typos.txt");

/// A [`Corrector`] driven by a list of fixed [`Rule`]s.
#[derive(Debug, Clone, Default)]
pub struct RuleCorrector {
    rules: Vec<Rule>,
    automaton: Option<AhoCorasick>,
    /// Length of the longest typo, in characters.
    longest: usize,
}

impl RuleCorrector {
    /// Compiles `rules` into a corrector.
    ///
    /// Empty typos and rules that change nothing are dropped. When a typo
    /// appears more than once, the last rule wins, as in a file read from
    /// top to bottom.
    pub fn new(rules: Vec<Rule>) -> Self {
        let mut index: HashMap<String, usize> = HashMap::with_capacity(rules.len());
        let mut kept: Vec<Rule> = Vec::with_capacity(rules.len());
        for rule in rules {
            if rule.from.is_empty() || rule.from == rule.to {
                continue;
            }
            match index.get(&rule.from) {
                Some(&i) => kept[i] = rule,
                None => {
                    index.insert(rule.from.clone(), kept.len());
                    kept.push(rule);
                }
            }
        }

        let longest = kept
            .iter()
            .map(|r| r.from.chars().count())
            .max()
            .unwrap_or(0);
        // Standard semantics report every match, overlapping ones included,
        // which the nearest-to-the-end search below needs.
        let automaton = (!kept.is_empty())
            .then(|| {
                AhoCorasick::builder()
                    .match_kind(MatchKind::Standard)
                    .build(kept.iter().map(|r| r.from.as_str()))
                    .ok()
            })
            .flatten();
        Self {
            rules: kept,
            automaton,
            longest,
        }
    }

    /// Returns the rules shipped with Typomend.
    pub fn builtin_rules() -> Vec<Rule> {
        let mut rules = vec![Rule::new("因該", "應該")];
        rules.extend(parse_rules(COMMON_TYPOS).0);
        rules
    }

    /// Returns the number of distinct rules in effect.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns `true` if no rule is in effect.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

impl Corrector for RuleCorrector {
    fn last_correction(&self, text: &str, within: usize) -> Option<Correction> {
        let automaton = self.automaton.as_ref()?;
        // Any match that ends in the last `within` characters starts inside
        // this tail.
        let offset = tail_start(text, within + self.longest);
        let tail = &text[offset..];
        // The match nearest the end wins; of those ending at the same place,
        // the longest, so 因該 beats a rule for 該 alone.
        let best = automaton
            .find_overlapping_iter(tail)
            .max_by_key(|m| (m.end(), m.len()))?;
        let rule = &self.rules[best.pattern().as_usize()];
        Some(Correction {
            start: offset + best.start(),
            from: rule.from.clone(),
            to: rule.to.clone(),
        })
    }
}

/// Returns the byte offset where the last `chars` characters of `text` begin,
/// walking back from the end so long documents cost nothing extra.
fn tail_start(text: &str, chars: usize) -> usize {
    if chars == 0 {
        return text.len();
    }
    text.char_indices()
        .rev()
        .nth(chars - 1)
        .map_or(0, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn corrector() -> RuleCorrector {
        RuleCorrector::new(RuleCorrector::builtin_rules())
    }

    #[test]
    fn finds_builtin_typo() {
        let text = "這個因該可以";
        let c = corrector().last_correction(text, 32).unwrap();
        assert_eq!((c.from.as_str(), c.to.as_str()), ("因該", "應該"));
        assert_eq!(&text[c.start..c.end()], "因該");
    }

    #[test]
    fn fixes_every_common_typo() {
        let (rules, errors) = parse_rules(COMMON_TYPOS);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(rules.len() > 200);
        let c = corrector();
        for rule in &rules {
            let text = format!("我覺得{}", rule.from);
            let fix = c.last_correction(&text, 32).unwrap();
            assert_eq!(
                (fix.from.as_str(), fix.to.as_str()),
                (rule.from.as_str(), rule.to.as_str())
            );
            // A rule must not turn one typo into another.
            assert_eq!(c.last_correction(&rule.to, 32), None, "{}", rule.to);
        }
    }

    #[test]
    fn returns_none_for_clean_text() {
        assert_eq!(corrector().last_correction("這個應該可以", 32), None);
    }

    #[test]
    fn prefers_occurrence_nearest_the_end() {
        let text = "因該，因該";
        let c = corrector().last_correction(text, 32).unwrap();
        assert_eq!(c.end(), text.len());
    }

    #[test]
    fn prefers_rule_matching_nearest_the_end() {
        let c = RuleCorrector::new(vec![Rule::new("因該", "應該"), Rule::new("在見", "再見")])
            .last_correction("因該說在見", 32)
            .unwrap();
        assert_eq!(c.from, "在見");
    }

    #[test]
    fn prefers_longer_typo_ending_at_the_same_place() {
        let c = RuleCorrector::new(vec![Rule::new("該", "x"), Rule::new("因該", "應該")])
            .last_correction("這個因該", 32)
            .unwrap();
        assert_eq!(c.from, "因該");
    }

    #[test]
    fn ignores_typos_far_from_the_end() {
        let text = format!("因該{}", "好".repeat(40));
        assert_eq!(corrector().last_correction(&text, 32), None);
        assert!(corrector().last_correction(&text, 40).is_some());
    }

    #[test]
    fn later_duplicate_wins_and_degenerate_rules_are_dropped() {
        let rules = vec![
            Rule::new("因該", "因為"),
            Rule::new("因該", "應該"),
            Rule::new("", "x"),
            Rule::new("同", "同"),
        ];
        let c = RuleCorrector::new(rules);
        assert_eq!(c.len(), 1);
        assert_eq!(c.last_correction("因該", 32).unwrap().to, "應該");
        assert_eq!(c.last_correction("同", 32), None);
    }

    #[test]
    fn handles_large_rule_lists_quickly() {
        let rules: Vec<Rule> = (0..200_000)
            .map(|i| Rule::new(format!("錯{i}字"), format!("對{i}字")))
            .chain(RuleCorrector::builtin_rules())
            .collect();
        let started = Instant::now();
        let corrector = RuleCorrector::new(rules);
        let built = started.elapsed();

        let document = format!("{}這個因該可以", "很長的文件內容。".repeat(50_000));
        let started = Instant::now();
        let found = corrector.last_correction(&document, 32);
        let looked_up = started.elapsed();

        assert_eq!(found.map(|c| c.to), Some("應該".to_string()));
        assert_eq!(
            corrector.last_correction(&document, 32).unwrap().end(),
            document.len() - "可以".len()
        );
        println!("200k rules: built in {built:?}, lookup in {looked_up:?}");
    }
}
