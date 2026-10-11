// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tests of real models on known typos.
//!
//! The tests are ignored by default because they need the models under
//! `models/` at the root of the repository. Fetch each one with
//!
//! ```text
//! cargo run --release -p typomend-scorer --example download -- <id> models/<id>
//! ```
//!
//! and run the tests with `cargo test -p typomend-scorer -- --ignored`.

use typomend_engine::Corrector;
use typomend_scorer::{CandidateScorer, Detector, Homophones, LmCorrector, Mode, Score};

/// Loads google-bert-base-chinese, scoring text as written.
fn scorer() -> CandidateScorer {
    CandidateScorer::load(model_dir("google-bert-base-chinese"), false)
        .expect("download google-bert-base-chinese first")
}

/// Returns the score with the highest probability.
fn best(scores: &[Score]) -> Score {
    *scores
        .iter()
        .max_by(|a, b| a.probability.total_cmp(&b.probability))
        .expect("at least one candidate")
}

/// Asserts that `expected` wins at `position` by at least `margin` times the
/// probability of the typed character, which is the first candidate.
fn assert_corrects(
    scorer: &CandidateScorer,
    text: &str,
    position: usize,
    candidates: &str,
    expected: char,
    margin: f32,
) {
    let candidates: Vec<char> = candidates.chars().collect();
    let scores = scorer
        .score_candidates(text, position, &candidates)
        .expect("scoring failed");
    let top = best(&scores);
    assert_eq!(top.candidate, expected, "{text}: {scores:?}");
    assert!(
        top.probability > scores[0].probability * margin,
        "{text}: {scores:?}"
    );
}

#[test]
#[ignore = "needs models/google-bert-base-chinese"]
fn corrects_common_typos() {
    let s = scorer();
    assert_corrects(
        &s,
        "因問我今天有點想睡覺所以我去喝咖啡",
        1,
        "問為位未味",
        '為',
        1000.0,
    );
    assert_corrects(&s, "我今天早上睡過投所以遲到了", 7, "投頭透偷", '頭', 100.0);
    assert_corrects(&s, "這個因該可以", 2, "因應音英", '應', 10.0);
    assert_corrects(&s, "我在做什摸", 4, "摸麼磨模", '麼', 100.0);
}

/// Masking a whitespace character is an error rather than a guess.
#[test]
#[ignore = "needs models/google-bert-base-chinese"]
fn rejects_whitespace_position() {
    assert!(scorer().score_candidates("因 問", 1, &['為']).is_err());
}

/// Returns the directory of the model `id` under `models/` at the root of the
/// repository.
fn model_dir(id: &str) -> String {
    format!("{}/../../models/{id}", env!("CARGO_MANIFEST_DIR"))
}

/// Builds a corrector over the model `id` with the built-in homophones.
fn corrector(id: &str, simplify: bool, mode: Mode, threshold: f32) -> LmCorrector {
    let scorer = CandidateScorer::load(model_dir(id), simplify).expect("model");
    LmCorrector::new(
        Detector::new(scorer, Homophones::builtin(), mode),
        threshold,
    )
}

/// Returns the typo and its fix that `corrector` finds in `text`, looking as
/// far back as the observer does by default.
fn fix(corrector: &LmCorrector, text: &str) -> Option<(String, String)> {
    corrector.last_correction(text, 32).map(|f| (f.from, f.to))
}

/// Wraps a typo and its fix in the shape [`fix`] returns.
fn pair(from: &str, to: &str) -> Option<(String, String)> {
    Some((from.to_owned(), to.to_owned()))
}

/// A typo in the last character typed is corrected even with nothing after
/// it, so stopping right after the typo does not leave it in place.
#[test]
#[ignore = "needs models/macbert4csc-base-chinese"]
fn corrects_a_typo_at_the_very_end() {
    let c = corrector(
        "macbert4csc-base-chinese",
        true,
        Mode::Hybrid { prefilter: 0.1 },
        300.0,
    );
    assert_eq!(fix(&c, "我今天去打籃求"), pair("求", "球"));
    assert_eq!(fix(&c, "我今天去打篮求"), pair("求", "球"));
    assert_eq!(fix(&c, "我今天去打籃球"), None);
}

#[test]
#[ignore = "needs models/google-bert-base-chinese"]
fn corrects_typos_in_context_and_leaves_clean_text() {
    let c = corrector("google-bert-base-chinese", false, Mode::Masked, 30.0);
    let found = c
        .last_correction("我今天早上睡過投所以", 32)
        .expect("correction");
    assert_eq!((found.from.as_str(), found.to.as_str()), ("投", "頭"));
    assert_eq!(found.start, "我今天早上睡過".len());
    assert_eq!(fix(&c, "我今天早上睡過頭所以遲到了"), None);
    assert_eq!(fix(&c, "這個因該可以，"), pair("因", "應"));
}

#[test]
#[ignore = "needs models/google-bert-base-chinese"]
fn keeps_the_script_of_the_text() {
    let c = corrector("google-bert-base-chinese", false, Mode::Masked, 30.0);
    assert_eq!(fix(&c, "我今天早上睡過投所以"), pair("投", "頭"));
    assert_eq!(fix(&c, "我今天早上睡过投所以"), pair("投", "头"));
}

/// The last characters are checked, but need a larger margin than ones
/// with context after them.
#[test]
#[ignore = "needs models/google-bert-base-chinese"]
fn asks_more_of_the_last_characters() {
    let scorer =
        CandidateScorer::load(model_dir("google-bert-base-chinese"), false).expect("model");
    let detector = Detector::new(scorer, Homophones::builtin(), Mode::Masked);
    let ratio = |text: &str| {
        detector
            .assess(text, 7..8)
            .expect("assess")
            .first()
            .map(|a| a.ratio)
            .expect("assessment")
    };
    let at_end = ratio("我今天早上睡過投");
    let in_context = ratio("我今天早上睡過投所以");
    // A threshold the typo passes with context after it, and passes at the
    // end only without the extra margin.
    let threshold = at_end.min(in_context) / 2.0;
    let c = corrector("google-bert-base-chinese", false, Mode::Masked, threshold);
    assert_eq!(fix(&c, "我今天早上睡過投所以"), pair("投", "頭"));
    // Short of the margin, the typo is only fixed if the model is far surer
    // of it with the text taken to end there.
    let ended = ratio("我今天早上睡過投。");
    let sure = at_end >= threshold * 10.0 || ended >= threshold * 100.0;
    let expected = if sure { pair("投", "頭") } else { None };
    assert_eq!(fix(&c, "我今天早上睡過投"), expected);
}

/// The verdicts shown on the Try page agree with what the corrector does.
#[test]
#[ignore = "needs models/macbert4csc-base-chinese"]
fn explains_what_it_corrects() {
    let c = corrector("macbert4csc-base-chinese", true, Mode::Masked, 3000.0);
    let text = "我要茶原因";
    let verdicts = c.explain(text).expect("explain");
    let passing: Vec<_> = verdicts.iter().filter(|v| v.passes).collect();
    assert_eq!(passing.len(), 1, "{verdicts:?}");
    let a = passing[0].assessment;
    assert_eq!((a.position, a.original, a.best), (2, '茶', '查'));
    assert_eq!(passing[0].required, 3000.0);
    assert_eq!(fix(&c, text), pair("茶", "查"));
    // The last two characters need ten times the ratio.
    assert!(
        verdicts
            .iter()
            .any(|v| v.assessment.position == 4 && v.required == 30000.0)
    );
}

/// A typo at the very end that falls short of the stricter threshold for the
/// last characters is still fixed when the model is sure of it once the text
/// is taken to end there.
#[test]
#[ignore = "needs models/macbert4csc-base-chinese"]
fn corrects_the_last_character_when_sure_at_the_end_of_the_text() {
    let c = corrector("macbert4csc-base-chinese", true, Mode::Masked, 3000.0);
    let text = "今天要去吃臭豆府";
    let last = c.explain(text).expect("explain").pop().expect("verdict");
    assert!(last.assessment.ratio < last.required, "{last:?}");
    assert!(last.passes && last.ended.is_some(), "{last:?}");
    assert_eq!(fix(&c, text), pair("府", "腐"));
    assert_eq!(fix(&c, "今天要去吃臭豆腐"), None);
}
