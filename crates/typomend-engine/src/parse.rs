// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The plain-text rule list format.
//!
//! One rule per line, `typo = correction`. Text around `=` is trimmed, so
//! spacing is free. Blank lines and lines starting with `#` are ignored, which
//! also lets the user switch a rule off by commenting it out. The split is on
//! the first `=`, so a correction may itself contain `=` but a typo may not.
//! A leading byte order mark, which Notepad writes, is skipped.

use crate::Rule;

/// A line that could not be read as a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Line number, counting from 1.
    pub line: usize,
    /// What is wrong with the line.
    pub kind: ParseErrorKind,
}

/// Why a line is not a valid rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// The line has no `=`.
    MissingSeparator,
    /// The typo or the correction is empty.
    EmptySide,
}

/// Reads a rule list in the format described in the module documentation.
///
/// Returns the rules in file order, together with the lines that could not
/// be read. Invalid lines are skipped and reported, so one mistake never
/// disables the rest of the list.
pub fn parse_rules(text: &str) -> (Vec<Rule>, Vec<ParseError>) {
    let mut rules = Vec::new();
    let mut errors = Vec::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let error = |kind| ParseError { line: i + 1, kind };
        let Some((from, to)) = line.split_once('=') else {
            errors.push(error(ParseErrorKind::MissingSeparator));
            continue;
        };
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty() || to.is_empty() {
            errors.push(error(ParseErrorKind::EmptySide));
            continue;
        }
        rules.push(Rule::new(from, to));
    }
    (rules, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rules_comments_and_blank_lines() {
        let text = "\u{feff}# header\r\n因該 = 應該\r\n\r\n  在見=再見  \n# 以後=已後\n";
        let (rules, errors) = parse_rules(text);
        assert_eq!(
            rules,
            vec![Rule::new("因該", "應該"), Rule::new("在見", "再見")]
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn reports_bad_lines_and_keeps_the_rest() {
        let (rules, errors) = parse_rules("因該 應該\n= 應該\n因該 =\n在見 = 再見\n");
        assert_eq!(rules, vec![Rule::new("在見", "再見")]);
        assert_eq!(
            errors,
            vec![
                ParseError {
                    line: 1,
                    kind: ParseErrorKind::MissingSeparator
                },
                ParseError {
                    line: 2,
                    kind: ParseErrorKind::EmptySide
                },
                ParseError {
                    line: 3,
                    kind: ParseErrorKind::EmptySide
                },
            ]
        );
    }

    #[test]
    fn splits_on_the_first_separator() {
        let (rules, _) = parse_rules("a = b = c");
        assert_eq!(rules, vec![Rule::new("a", "b = c")]);
    }
}
