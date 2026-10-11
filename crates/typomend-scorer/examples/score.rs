// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Prints candidate scores for a masked position.
//!
//! ```text
//! cargo run --release -p typomend-scorer --example score
//! cargo run --release -p typomend-scorer --example score -- <text> <position> <candidates>
//! ```
//!
//! Without arguments it scores a fixed set of typos and correct sentences.
//! The first candidate is treated as the character that was typed. The model
//! directory defaults to `models/google-bert-base-chinese`; `TYPOMEND_MODEL`
//! overrides it, and setting `TYPOMEND_SIMPLIFY` scores the text in its
//! Simplified form.

use std::time::Instant;

use anyhow::{Context, Result, bail};
use typomend_scorer::CandidateScorer;

/// Texts with the position to mask and the candidates for it, typed
/// character first.
const SAMPLES: &[(&str, usize, &str)] = &[
    ("因問我今天有點想睡覺所以我去喝咖啡", 1, "問為位未味"),
    ("我今天早上睡過投所以遲到了", 7, "投頭透偷"),
    ("這個因該可以", 2, "因應音英"),
    ("我在做什摸", 4, "摸麼磨模"),
    // Correct text: the typed character should stay on top.
    ("因為我今天有點想睡覺所以我去喝咖啡", 1, "為問位未味"),
    ("我今天早上睡過頭所以遲到了", 7, "頭投透偷"),
];

fn main() -> Result<()> {
    let dir = std::env::var("TYPOMEND_MODEL")
        .unwrap_or_else(|_| "models/google-bert-base-chinese".into());
    let started = Instant::now();
    let scorer = CandidateScorer::load(&dir, std::env::var_os("TYPOMEND_SIMPLIFY").is_some())
        .with_context(|| format!("loading model from {dir}"))?;
    println!("loaded {dir} in {:?}\n", started.elapsed());

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            for &(text, position, candidates) in SAMPLES {
                score(&scorer, text, position, candidates)?;
            }
        }
        [text, position, candidates] => {
            score(
                &scorer,
                text,
                position.parse().context("position")?,
                candidates,
            )?;
        }
        _ => bail!("usage: score [<text> <position> <candidates>]"),
    }
    Ok(())
}

/// Prints the scores of `candidates` at `position` of `text`, each with its
/// share of their total, and whether the best one would replace the first.
fn score(scorer: &CandidateScorer, text: &str, position: usize, candidates: &str) -> Result<()> {
    let candidates: Vec<char> = candidates.chars().collect();
    if candidates.is_empty() {
        bail!("no candidates");
    }
    let started = Instant::now();
    let mut scores = scorer.score_candidates(text, position, &candidates)?;
    let elapsed = started.elapsed();

    let typed = scores[0];
    let total: f32 = scores.iter().map(|s| s.probability).sum();
    scores.sort_by(|a, b| b.probability.total_cmp(&a.probability));
    let best = scores[0];

    println!("{text}  position {position}  ({elapsed:?})");
    for s in &scores {
        let share = if total > 0.0 {
            s.probability / total
        } else {
            0.0
        };
        println!(
            "  {}  {:.6}  share {:.4}",
            s.candidate, s.probability, share
        );
    }
    if best.candidate == typed.candidate {
        println!("  -> keep {}\n", typed.candidate);
    } else {
        let ratio = best.probability / typed.probability.max(f32::MIN_POSITIVE);
        println!(
            "  -> {} to {}, {ratio:.0}x more likely\n",
            typed.candidate, best.candidate
        );
    }
    Ok(())
}
