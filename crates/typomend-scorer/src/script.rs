// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Character-level Traditional to Simplified conversion.
//!
//! Most Chinese masked language models saw far more Simplified than Traditional
//! text, so scoring the Simplified form of a Traditional sentence can be
//! more accurate. Conversion is one character to one character, which keeps
//! positions aligned; phrase-level rules would be more faithful but could
//! change the length.
//!
//! A character with several Simplified forms, such as 於 with 于 and 於,
//! becomes the most common one. Leaving it alone is worse: a model trained
//! on Simplified text has rarely seen 於 and finds 于 thousands of times
//! likelier, which the corrector takes for a typo in about one correct
//! sentence in thirty.
//!
//! The table comes from OpenCC TSCharacters.txt under the Apache License
//! 2.0: <https://github.com/BYVoid/OpenCC>.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Lines of a Traditional character and its Simplified forms, tab and space
/// separated, the most common form first.
const T2S: &str = include_str!("../data/t2s.txt");

/// Returns the Simplified forms of each Traditional character, the most
/// common first. The table is parsed on first use.
fn table() -> &'static HashMap<char, Vec<char>> {
    static TABLE: OnceLock<HashMap<char, Vec<char>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        T2S.lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let (t, forms) = l.split_once('\t')?;
                let mut t = t.chars();
                let (Some(t), None) = (t.next(), t.next()) else {
                    return None;
                };
                Some((
                    t,
                    forms.split(' ').filter_map(|f| f.chars().next()).collect(),
                ))
            })
            .collect()
    })
}

/// Returns the most common Simplified form of `c`, or `c` when it has none.
pub fn to_simplified(c: char) -> char {
    table()
        .get(&c)
        .and_then(|forms| forms.first())
        .copied()
        .unwrap_or(c)
}

/// Returns whether `s` is a Simplified form of `t`, as 于 is of 於.
pub fn is_simplified_form(s: char, t: char) -> bool {
    table().get(&t).is_some_and(|forms| forms.contains(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_single_characters() {
        assert_eq!(to_simplified('為'), '为');
        assert_eq!(to_simplified('頭'), '头');
        assert_eq!(to_simplified('我'), '我');
        assert_eq!(to_simplified('，'), '，');
    }

    #[test]
    fn takes_the_most_common_of_several_forms() {
        assert_eq!(to_simplified('於'), '于');
        assert_eq!(to_simplified('麼'), '么');
        // 瞭 stays 瞭 in 瞭望 and only becomes 了 in 瞭解.
        assert_eq!(to_simplified('瞭'), '瞭');
        assert!(is_simplified_form('了', '瞭'));
        assert!(is_simplified_form('于', '於'));
        assert!(!is_simplified_form('於', '于'));
    }
}
