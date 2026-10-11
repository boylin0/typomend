// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test sets and the typing simulation used to measure a corrector.
//!
//! Each test set holds pairs of a typed sentence with one typo and the
//! intended sentence. The simulation types every sentence a few characters
//! at a time, the way an input method commits phrases, asks the corrector
//! after every commit and applies what it returns, just as Typomend does. A
//! typed sentence counts as fixed when it ends up as the intended one; an
//! intended sentence counts as a false positive when it ends up changed.
//!
//! This measures the whole policy, thresholds and the wait for context
//! included, rather than the model alone; the `bench` example also sweeps
//! thresholds over whole sentences to choose them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use typomend_engine::{Correction, Corrector};

/// Characters committed at a time. Input methods commit a word or phrase,
/// usually one or two characters, at a time.
const COMMIT: usize = 2;

/// Characters before the caret the corrector may change, as in the default
/// settings.
const WITHIN: usize = 32;

/// Corrections applied after one commit before moving on, so a corrector
/// that keeps changing its mind cannot loop forever.
const MAX_FIXES_PER_COMMIT: usize = 3;

/// Pairs typed one at a time to measure how long a check takes. Six give
/// the same mean time per check as twelve to within a few milliseconds, and
/// typing them takes about as long as typing all the others side by side.
const TIMED: usize = 6;

/// One typo: the typed sentence and the intended one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// The sentence as typed, with the typo.
    pub typed: String,
    /// The sentence as meant.
    pub intended: String,
    /// Character position of the typo.
    pub position: usize,
    /// The character that was meant there.
    pub correct: char,
}

/// A bundled test set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestSet {
    /// Short name, such as `zh-TW`.
    pub id: &'static str,
    /// What the sentences are, in English.
    pub description: &'static str,
    text: &'static str,
}

impl TestSet {
    /// Returns the pairs of the set.
    pub fn cases(&self) -> Vec<Case> {
        // The bundled sets are checked by a test, so they always parse.
        parse(self.text).unwrap_or_default()
    }
}

/// The test sets bundled with the crate.
///
/// The results in [`crate::MODELS`] are indexed by the order of this list.
/// Measure every model again after adding or reordering a set.
pub const TEST_SETS: &[TestSet] = &[
    TestSet {
        id: "zh-TW",
        description: "Traditional Chinese, Taiwan",
        text: include_str!("../bench/zh-TW.tsv"),
    },
    TestSet {
        id: "zh-CN",
        description: "Simplified Chinese, mainland",
        text: include_str!("../bench/zh-CN.tsv"),
    },
    TestSet {
        id: "zh-TW-mixed",
        description: "Traditional Chinese with English words and numbers",
        text: include_str!("../bench/zh-TW-mixed.tsv"),
    },
];

/// Parses a test set: one pair per line, typed and intended text separated
/// by a tab, differing in exactly one character. Lines starting with `#`
/// are comments.
///
/// # Errors
///
/// Returns a message naming the first malformed line.
pub fn parse(text: &str) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let Some((typed, intended)) = line.split_once('\t') else {
            return Err(format!("line {}: expected two columns", n + 1));
        };
        let a: Vec<char> = typed.chars().collect();
        let b: Vec<char> = intended.chars().collect();
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a.get(i) != b.get(i)).collect();
        if a.len() != b.len() || diff.len() != 1 {
            return Err(format!(
                "line {}: sentences must differ in exactly one character",
                n + 1
            ));
        }
        cases.push(Case {
            typed: typed.to_owned(),
            intended: intended.to_owned(),
            position: diff[0],
            correct: b[diff[0]],
        });
    }
    Ok(cases)
}

impl Case {
    /// Returns how many characters follow the typo.
    pub fn right(&self) -> usize {
        self.typed.chars().count() - self.position - 1
    }
}

/// Names of the groups [`Outcome::by_right`] counts typos in, by how many
/// characters follow the typo. The corrector has the least to go on at the
/// end of the text, so a typo there is the hardest to fix.
pub const RIGHT_GROUPS: [&str; 4] = ["0", "1-2", "3-5", "6+"];

/// Returns the index in [`RIGHT_GROUPS`] of the group `case` falls in.
fn right_group(case: &Case) -> usize {
    match case.right() {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

/// Typos in one group of [`RIGHT_GROUPS`], and how many were fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Typos fixed.
    pub fixed: usize,
    /// Typos in the group.
    pub pairs: usize,
}

impl Tally {
    /// Returns the share of the typos that were fixed, from 0 to 1.
    pub fn fixed_rate(&self) -> f32 {
        self.fixed as f32 / self.pairs.max(1) as f32
    }
}

/// What the typing simulation found.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Outcome {
    /// Typed sentences that ended up as intended.
    pub fixed: usize,
    /// Intended sentences that ended up changed.
    pub false_positives: usize,
    /// Pairs typed.
    pub pairs: usize,
    /// Typed sentences per group of [`RIGHT_GROUPS`].
    pub by_right: [Tally; RIGHT_GROUPS.len()],
    /// Corrector calls that were timed. Only the pairs typed one at a time
    /// are timed; see [`simulate_typing`].
    pub calls: usize,
    /// Time the timed calls took in total.
    pub busy: Duration,
    /// Time the slowest timed call took.
    pub slowest: Duration,
}

impl Outcome {
    /// Returns the share of typed sentences that were fixed, from 0 to 1.
    pub fn fixed_rate(&self) -> f32 {
        self.fixed as f32 / self.pairs.max(1) as f32
    }

    /// Adds the result of typing the pair `case`.
    fn count(&mut self, case: &Case, fixed: bool, changed: bool) {
        self.fixed += usize::from(fixed);
        self.false_positives += usize::from(changed);
        let tally = &mut self.by_right[right_group(case)];
        tally.fixed += usize::from(fixed);
        tally.pairs += 1;
    }

    /// Returns the share of intended sentences that were changed, from 0
    /// to 1.
    pub fn false_positive_rate(&self) -> f32 {
        self.false_positives as f32 / self.pairs.max(1) as f32
    }

    /// Returns the mean time of a timed corrector call.
    pub fn mean_call(&self) -> Duration {
        self.busy / u32::try_from(self.calls.max(1)).unwrap_or(u32::MAX)
    }
}

/// Types every sentence of `cases` through `corrector`; see the module
/// documentation.
///
/// A few pairs spread over the set are typed one at a time first, and only
/// they are timed: calls made side by side compete for the processor, so
/// their times would not be what a single check takes while typing. The
/// rest are typed by `jobs` threads at once, each doing its model work on
/// its own thread. Splitting every matrix product over all processors as
/// well would make the threads get in each other's way, and the run would
/// take half as long again.
///
/// `progress` is called with the number of pairs done after each pair and
/// stops the run by returning `false`, in which case `None` is returned.
pub fn simulate_typing(
    corrector: &dyn Corrector,
    cases: &[Case],
    jobs: usize,
    progress: &mut dyn FnMut(usize) -> bool,
) -> Option<Outcome> {
    let mut outcome = Outcome {
        pairs: cases.len(),
        ..Outcome::default()
    };
    let step = cases.len().div_ceil(TIMED).max(1);
    let (timed, rest): (Vec<_>, Vec<_>) =
        cases.iter().enumerate().partition(|(i, _)| i % step == 0);
    let (timed, rest): (Vec<&Case>, Vec<&Case>) = (
        timed.into_iter().map(|(_, case)| case).collect(),
        rest.into_iter().map(|(_, case)| case).collect(),
    );
    let mut done = 0;
    for case in timed {
        let (fixed, changed) = type_pair(corrector, case, &mut outcome);
        outcome.count(case, fixed, changed);
        done += 1;
        if !progress(done) {
            return None;
        }
    }

    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, rest.len().max(1)) {
            let sender = sender.clone();
            let (next, stop, rest) = (&next, &stop, &rest);
            scope.spawn(move || {
                let work = || {
                    // Calls made side by side are not timed; see the
                    // function documentation.
                    let mut untimed = Outcome::default();
                    while !stop.load(Ordering::Relaxed) {
                        let Some(case) = rest.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        let (fixed, changed) = type_pair(corrector, case, &mut untimed);
                        if sender.send((case, fixed, changed)).is_err() {
                            break;
                        }
                    }
                };
                // candle splits matrix products over the Rayon pool it runs
                // in, so a pool of one thread keeps them on this thread.
                match rayon::ThreadPoolBuilder::new().num_threads(1).build() {
                    Ok(pool) => pool.install(work),
                    Err(_) => work(),
                }
            });
        }
        drop(sender);
        for (case, fixed, changed) in receiver {
            outcome.count(case, fixed, changed);
            done += 1;
            if !progress(done) {
                stop.store(true, Ordering::Relaxed);
                break;
            }
        }
    });
    (!stop.into_inner()).then_some(outcome)
}

/// Types both sentences of `case` and returns whether the typed one was
/// fixed and whether the intended one was changed.
fn type_pair(corrector: &dyn Corrector, case: &Case, outcome: &mut Outcome) -> (bool, bool) {
    let (typed, intended) = type_both(corrector, case, outcome);
    (typed == case.intended, intended != case.intended)
}

/// Types both sentences of `case`.
///
/// The two read the same up to the typo, so the corrector is asked about
/// each text only once and its answer reused for the other sentence. That
/// saves about a third of the calls without changing any result.
fn type_both(corrector: &dyn Corrector, case: &Case, outcome: &mut Outcome) -> (String, String) {
    let mut answers = HashMap::new();
    (
        type_text(corrector, &case.typed, outcome, &mut answers),
        type_text(corrector, &case.intended, outcome, &mut answers),
    )
}

/// Types both sentences of `case` as [`simulate_typing`] does and returns
/// what each ended up as, to see what went wrong with a pair.
pub fn type_case(corrector: &dyn Corrector, case: &Case) -> (String, String) {
    type_both(corrector, case, &mut Outcome::default())
}

/// Types `text` a commit at a time, applying corrections as they come, and
/// returns what ends up in the document. `answers` holds what the corrector
/// said about each text it was asked about; only calls not answered from it
/// are counted and timed.
fn type_text(
    corrector: &dyn Corrector,
    text: &str,
    outcome: &mut Outcome,
    answers: &mut HashMap<String, Option<Correction>>,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut document = String::new();
    for commit in chars.chunks(COMMIT) {
        document.extend(commit);
        for _ in 0..MAX_FIXES_PER_COMMIT {
            let fix = answers
                .entry(document.clone())
                .or_insert_with(|| {
                    let started = Instant::now();
                    let fix = corrector.last_correction(&document, WITHIN);
                    let took = started.elapsed();
                    outcome.calls += 1;
                    outcome.busy += took;
                    outcome.slowest = outcome.slowest.max(took);
                    fix
                })
                .clone();
            let Some(fix) = fix else {
                break;
            };
            if document.get(fix.start..fix.end()) != Some(fix.from.as_str()) {
                break;
            }
            document.replace_range(fix.start..fix.end(), &fix.to);
        }
    }
    document
}

#[cfg(test)]
mod tests {
    use typomend_engine::{Correction, RuleCorrector};

    use super::*;

    #[test]
    fn bundled_sets_parse() {
        for set in TEST_SETS {
            let cases = parse(set.text).unwrap();
            assert!(cases.len() >= 40, "{} has {} pairs", set.id, cases.len());
        }
    }

    #[test]
    fn rejects_pairs_that_do_not_differ_by_one() {
        assert!(parse("abc\tabc\n").is_err());
        assert!(parse("abc\txyz\n").is_err());
        assert!(parse("abc\n").is_err());
        assert_eq!(parse("# note\nab\tac\n").unwrap()[0].correct, 'c');
    }

    #[test]
    fn simulates_typing_with_a_rule_corrector() {
        let rules = RuleCorrector::new(vec![typomend_engine::Rule::new("因該", "應該")]);
        let cases = parse("這個因該可以\t這個應該可以\n我今天很好\t我今天很號\n").unwrap();
        let outcome = simulate_typing(&rules, &cases, 4, &mut |_| true).unwrap();
        // The rule fixes the first pair and leaves the second one alone in
        // both directions: 很號 stays a typo and 很好 stays unchanged.
        assert_eq!(outcome.fixed, 1);
        // Three characters follow the typo 因, and none follow 好.
        assert_eq!(outcome.by_right[2], Tally { fixed: 1, pairs: 1 });
        assert_eq!(outcome.by_right[0], Tally { fixed: 0, pairs: 1 });
        assert_eq!(outcome.false_positives, 0);
        assert!(outcome.calls > 0);
    }

    #[derive(Debug)]
    struct Flip;

    impl Corrector for Flip {
        fn last_correction(&self, text: &str, _within: usize) -> Option<Correction> {
            // Swaps the last character back and forth forever.
            let (start, c) = text.char_indices().last()?;
            let to = if c == 'a' { "b" } else { "a" };
            Some(Correction {
                start,
                from: c.to_string(),
                to: to.into(),
            })
        }
    }

    #[test]
    fn stops_a_corrector_that_never_settles() {
        let cases = parse("ab\tac\n").unwrap();
        let outcome = simulate_typing(&Flip, &cases, 4, &mut |_| true).unwrap();
        // Flip only ever sees ab, aa and ac, and is asked about each once.
        assert_eq!(outcome.calls, 3);
        assert_eq!(outcome.fixed + outcome.false_positives, 1);
    }

    /// Never finds anything and counts how often it is asked.
    #[derive(Debug, Default)]
    struct Counting(AtomicUsize);

    impl Corrector for Counting {
        fn last_correction(&self, _text: &str, _within: usize) -> Option<Correction> {
            self.0.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    #[test]
    fn asks_once_about_the_text_both_sentences_share() {
        // 我今天 is typed in both sentences of the pair before they part.
        let cases = parse("我今天去打籃求\t我今天去打籃球\n").unwrap();
        let counting = Counting::default();
        let outcome = simulate_typing(&counting, &cases, 1, &mut |_| true).unwrap();
        // Commits of two: 我今, 我今天去, 我今天去打籃 are shared, then each
        // sentence ends differently.
        assert_eq!(counting.0.load(Ordering::Relaxed), 5);
        assert_eq!(outcome.calls, 5);
    }

    #[test]
    fn counts_the_same_with_any_number_of_threads() {
        let rules = RuleCorrector::new(vec![typomend_engine::Rule::new("因該", "應該")]);
        let cases = TEST_SETS[0].cases();
        let one = simulate_typing(&rules, &cases, 1, &mut |_| true).unwrap();
        let many = simulate_typing(&rules, &cases, 8, &mut |_| true).unwrap();
        assert_eq!(
            (one.fixed, one.false_positives),
            (many.fixed, many.false_positives)
        );
        assert_eq!(many.pairs, cases.len());
    }

    #[test]
    fn stops_when_cancelled_while_typing_side_by_side() {
        let rules = RuleCorrector::default();
        let cases = TEST_SETS[0].cases();
        let mut last = 0;
        let outcome = simulate_typing(&rules, &cases, 8, &mut |done| {
            last = done;
            done < TIMED + 5
        });
        assert!(outcome.is_none());
        assert_eq!(last, TIMED + 5);
    }

    #[test]
    fn stops_when_cancelled() {
        let rules = RuleCorrector::default();
        let cases = parse("ab\tac\ncd\tce\n").unwrap();
        assert!(simulate_typing(&rules, &cases, 4, &mut |done| done < 1).is_none());
    }
}
