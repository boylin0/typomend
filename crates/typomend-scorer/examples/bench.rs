// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compares models, scripts and scoring modes on the typo test sets.
//!
//! ```text
//! cargo run --release -p typomend-scorer --example bench -- [options]
//!
//! --models a,b       model directories under --dir (default: all)
//! --dir PATH         where the models are (default: models)
//! --modes m,o,h      masked, one-pass, hybrid (default: all three)
//! --scripts t,s      score as written, as Simplified (default: both)
//! --sets a,b         bundled test sets: zh-TW, zh-CN, zh-TW-mixed
//!                    (default: zh-TW)
//! --set PATH         a test set file instead of the bundled ones; may be
//!                    given more than once
//! --right N          keep only N characters after the typo, as when the
//!                    user is still typing (default: whole sentence)
//! --prefilter R      hybrid mode: ratio that triggers the masked pass
//! --typing           simulate typing through the corrector instead of
//!                    sweeping thresholds; see typomend_scorer::bench
//! --threshold R      with --typing: threshold to use instead of the one
//!                    recommended for the model and mode
//! --jobs N           with --typing: sentences typed at once
//!                    (default: number of logical processors)
//! --misses           with --typing: also list the typos left unfixed and
//!                    the correct sentences changed
//! --rules            with --typing: apply the built-in rules too, as the
//!                    language model engine does
//! ```
//!
//! The threshold sweep checks every position of each whole sentence. A typed
//! sentence counts as fixed when the strongest finding above the threshold
//! is the right character at the right position; an intended sentence counts
//! as a false positive when any finding passes the threshold. Typing instead
//! feeds each sentence to the corrector a few characters at a time and
//! applies its corrections, as Typomend does while the user types.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use typomend_engine::{Correction, Corrector, RuleCorrector};
use typomend_scorer::bench::{self, Case, Outcome, RIGHT_GROUPS, TEST_SETS};
use typomend_scorer::{
    Assessment, CandidateScorer, Detector, Homophones, LmCorrector, MODELS, Mode, ModeKind, Script,
};

/// Thresholds the sweep reports a column for.
const THRESHOLDS: &[f32] = &[2.0, 5.0, 10.0, 30.0, 100.0, 300.0, 1000.0];

/// Highest false positive rate a threshold may have to be reported as the
/// best one.
const MAX_FP: f32 = 0.02;

/// The command line options described in the module documentation.
struct Options {
    models: Vec<String>,
    dir: PathBuf,
    /// Each mode with the name it is reported under.
    modes: Vec<(String, Mode)>,
    /// Whether to score as Simplified, once per script to compare.
    scripts: Vec<bool>,
    /// Each test set with the name it is reported under.
    sets: Vec<(String, Vec<Case>)>,
    /// Characters kept after the typo, `None` for the whole sentence.
    right: Option<usize>,
    typing: bool,
    threshold: Option<f32>,
    jobs: usize,
    misses: bool,
    rules: bool,
}

/// The built-in rules and a language model, combined as Typomend combines
/// them: the correction nearer the end of the text wins, and the rule wins a
/// tie.
struct WithRules<'a> {
    rules: RuleCorrector,
    lm: &'a dyn Corrector,
}

impl Corrector for WithRules<'_> {
    fn last_correction(&self, text: &str, within: usize) -> Option<Correction> {
        let rule = self.rules.last_correction(text, within);
        match (rule, self.lm.last_correction(text, within)) {
            (Some(r), Some(l)) if l.end() > r.end() => Some(l),
            (Some(r), _) => Some(r),
            (None, l) => l,
        }
    }
}

/// Runs the typing simulation with `--typing`, the threshold sweep otherwise.
fn main() -> Result<()> {
    let options = parse_args()?;
    if options.typing {
        typing(&options)
    } else {
        sweep(&options)
    }
}

/// Prints, per test set, the fix and false positive rates of every model,
/// script and mode at each of [`THRESHOLDS`], and the best threshold within
/// [`MAX_FP`].
fn sweep(options: &Options) -> Result<()> {
    for (set, cases) in &options.sets {
        let cases: Vec<Case> = cases.iter().map(|c| cut(c, options.right)).collect();
        println!(
            "\n{set}: {} pairs; right context: {}\n",
            cases.len(),
            options.right.map_or("full".into(), |n| n.to_string())
        );
        print!("| model | script | mode | ms/sent | rank |");
        for t in THRESHOLDS {
            print!(" >={t} |");
        }
        println!(" best (FP<=2%) |");
        print!("|---|---|---|---:|---:|");
        for _ in THRESHOLDS {
            print!("---|");
        }
        println!("---|");

        for model in &options.models {
            for &simplify in &options.scripts {
                for (mode_name, mode) in &options.modes {
                    let scorer = load(options, model, simplify)?;
                    let detector = Detector::new(scorer, homophones(set), *mode);
                    let row = evaluate(&detector, &cases)?;
                    let script = if simplify { "s" } else { "t" };
                    print!(
                        "| {model} | {script} | {mode_name} | {:.0} | {:.0}% |",
                        row.ms_per_sentence,
                        row.rank * 100.0
                    );
                    let mut best: Option<(f32, f32, f32)> = None;
                    for &t in THRESHOLDS {
                        let (recall, fp) = row.at(t);
                        print!(" {:.0}/{:.0} |", recall * 100.0, fp * 100.0);
                        if fp <= MAX_FP && best.is_none_or(|b| recall > b.1) {
                            best = Some((t, recall, fp));
                        }
                    }
                    match best {
                        Some((t, r, f)) => {
                            println!(" {:.0}% @{t} ({:.0}% FP) |", r * 100.0, f * 100.0);
                        }
                        None => println!(" - |"),
                    }
                }
            }
        }
    }
    println!("\ncells: recall / false positive rate, in percent");
    Ok(())
}

/// Prints how typing each test set through the corrector of every model,
/// script and mode turns out, at the recommended threshold unless
/// `--threshold` gives one. A model missing from [`MODELS`] gets 30.
fn typing(options: &Options) -> Result<()> {
    println!(
        "| model | script | mode | threshold | set | fixed | false positives | ms/call | slowest |{}",
        right_header()
    );
    println!(
        "|---|---|---|---:|---|---:|---:|---:|---:|{}",
        "---:|".repeat(RIGHT_GROUPS.len())
    );
    for model in &options.models {
        for &simplify in &options.scripts {
            for (mode_name, mode) in &options.modes {
                let kind = match mode {
                    Mode::Masked => ModeKind::Masked,
                    Mode::Hybrid { .. } => ModeKind::Hybrid,
                    Mode::OnePass => ModeKind::OnePass,
                };
                let threshold = options
                    .threshold
                    .or_else(|| {
                        MODELS
                            .iter()
                            .find(|m| m.id == model)
                            .map(|m| m.threshold(kind))
                    })
                    .unwrap_or(30.0);
                let script = if simplify { "s" } else { "t" };
                for (set, cases) in &options.sets {
                    let scorer = load(options, model, simplify)?;
                    let detector = Detector::new(scorer, homophones(set), *mode);
                    let lm = LmCorrector::new(detector, threshold);
                    let with_rules = WithRules {
                        rules: RuleCorrector::new(RuleCorrector::builtin_rules()),
                        lm: &lm,
                    };
                    let corrector: &dyn Corrector = if options.rules { &with_rules } else { &lm };
                    let outcome =
                        bench::simulate_typing(corrector, cases, options.jobs, &mut |_| true)
                            .context("simulation stopped")?;
                    println!(
                        "| {model} | {script} | {mode_name} | {threshold} | {set} | {:.0}% | {:.0}% | {:.0} | {:.0} |{}",
                        outcome.fixed_rate() * 100.0,
                        outcome.false_positive_rate() * 100.0,
                        outcome.mean_call().as_secs_f64() * 1000.0,
                        outcome.slowest.as_secs_f64() * 1000.0,
                        right_cells(&outcome),
                    );
                    if options.misses {
                        print_misses(corrector, cases, options.jobs);
                    }
                }
            }
        }
    }
    if options.misses {
        println!(
            "
misses: typed, intended, what typing ended up as"
        );
    }
    Ok(())
}

/// Header cells for the fix rate per group of characters after the typo.
fn right_header() -> String {
    RIGHT_GROUPS
        .iter()
        .map(|g| format!(" right {g} |"))
        .collect()
}

/// Row cells under [`right_header`]: the fix rate and number of pairs in each
/// group.
fn right_cells(outcome: &Outcome) -> String {
    outcome
        .by_right
        .iter()
        .map(|t| format!(" {:.0}% ({}) |", t.fixed_rate() * 100.0, t.pairs))
        .collect()
}

/// Lists the pairs the corrector got wrong, typing them again on `jobs`
/// threads.
fn print_misses(corrector: &dyn Corrector, cases: &[Case], jobs: usize) {
    let results: Vec<(String, String)> = std::thread::scope(|scope| {
        let chunk = cases.len().div_ceil(jobs.max(1)).max(1);
        let handles: Vec<_> = cases
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|case| bench::type_case(corrector, case))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("typing thread panicked"))
            .collect()
    });
    for (case, (typed, intended)) in cases.iter().zip(results) {
        if typed != case.intended {
            println!("  unfixed	{}	{}	{typed}", case.typed, case.intended);
        }
        if intended != case.intended {
            println!("  changed		{}	{intended}", case.intended);
        }
    }
}

/// Returns the candidate generator for the test set named `set`.
///
/// A sentence that does not tell its script is taken to be in the script the
/// set's name implies, as Typomend takes it to be in the user's.
fn homophones(set: &str) -> Homophones {
    let script = if set.contains("zh-CN") {
        Script::Simplified
    } else {
        Script::Traditional
    };
    Homophones::builtin().assuming(script)
}

/// Loads `model` from the models directory, scoring as Simplified when
/// `simplify` is set.
fn load(options: &Options, model: &str, simplify: bool) -> Result<CandidateScorer> {
    let dir = options.dir.join(model);
    CandidateScorer::load(&dir, simplify).with_context(|| format!("loading {}", dir.display()))
}

/// Keeps only `right` characters after the typo.
fn cut(case: &Case, right: Option<usize>) -> Case {
    let Some(right) = right else {
        return case.clone();
    };
    let keep = case.position + 1 + right;
    Case {
        typed: case.typed.chars().take(keep).collect(),
        intended: case.intended.chars().take(keep).collect(),
        ..case.clone()
    }
}

/// The assessments of one model, script and mode over a test set, from which
/// [`Row::at`] works out the rates at any threshold.
struct Row {
    ms_per_sentence: f64,
    /// Share of typos where the intended character ranks first at the typo.
    rank: f32,
    /// Per typed sentence: its assessments, the typo's position and the
    /// intended character.
    typed: Vec<(Vec<Assessment>, usize, char)>,
    /// Per intended sentence: its assessments.
    clean: Vec<Vec<Assessment>>,
}

impl Row {
    /// Returns the fix rate and the false positive rate at `threshold`.
    fn at(&self, threshold: f32) -> (f32, f32) {
        let top = |found: &[Assessment]| {
            found
                .iter()
                .filter(|a| a.ratio >= threshold)
                .max_by(|a, b| a.ratio.total_cmp(&b.ratio))
                .copied()
        };
        let fixed = self
            .typed
            .iter()
            .filter(|(found, p, c)| top(found).is_some_and(|a| a.position == *p && a.best == *c))
            .count();
        let fp = self
            .clean
            .iter()
            .filter(|found| top(found).is_some())
            .count();
        (
            fixed as f32 / self.typed.len() as f32,
            fp as f32 / self.clean.len() as f32,
        )
    }
}

/// Assesses every position of the typed and the intended sentence of each
/// case.
fn evaluate(detector: &Detector, cases: &[Case]) -> Result<Row> {
    let started = Instant::now();
    let mut typed = Vec::new();
    let mut clean = Vec::new();
    for case in cases {
        let found = detector.assess(&case.typed, 0..case.typed.chars().count())?;
        typed.push((found, case.position, case.correct));
        clean.push(detector.assess(&case.intended, 0..case.intended.chars().count())?);
    }
    let ms_per_sentence = started.elapsed().as_secs_f64() * 1000.0 / (2 * cases.len()) as f64;
    let ranked = typed
        .iter()
        .filter(|(found, p, c)| {
            found
                .iter()
                .any(|a| a.position == *p && a.best == *c && a.ratio > 1.0)
        })
        .count();
    Ok(Row {
        ms_per_sentence,
        rank: ranked as f32 / cases.len() as f32,
        typed,
        clean,
    })
}

/// Parses the command line into [`Options`].
///
/// Flags that take a value read the next argument. Without `--models`, every
/// model installed under the models directory is used.
///
/// # Errors
///
/// Returns an error for an unknown flag, mode or test set, a missing or
/// unparsable value, or a test set file that cannot be read.
fn parse_args() -> Result<Options> {
    let mut options = Options {
        models: Vec::new(),
        dir: PathBuf::from("models"),
        modes: Vec::new(),
        scripts: vec![false, true],
        sets: Vec::new(),
        right: None,
        typing: false,
        threshold: None,
        jobs: std::thread::available_parallelism().map_or(1, NonZero::get),
        misses: false,
        rules: false,
    };
    let mut modes = "m,o,h".to_owned();
    let mut prefilter = 0.1;
    let mut sets = "zh-TW".to_owned();
    let mut files: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--typing" {
            options.typing = true;
            continue;
        }
        if flag == "--misses" {
            options.misses = true;
            continue;
        }
        if flag == "--rules" {
            options.rules = true;
            continue;
        }
        let value = args
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--models" => options.models = value.split(',').map(str::to_owned).collect(),
            "--dir" => options.dir = value.into(),
            "--modes" => modes = value,
            "--scripts" => {
                options.scripts = value.split(',').map(|s| s == "s").collect();
            }
            "--sets" => sets = value,
            "--set" => files.push(value.into()),
            "--right" => options.right = Some(value.parse()?),
            "--prefilter" => prefilter = value.parse()?,
            "--threshold" => options.threshold = Some(value.parse()?),
            "--jobs" => options.jobs = value.parse()?,
            _ => bail!("unknown option {flag}"),
        }
    }
    for m in modes.split(',') {
        options.modes.push(match m {
            "m" => ("masked".into(), Mode::Masked),
            "o" => ("one-pass".into(), Mode::OnePass),
            "h" => (format!("hybrid {prefilter}"), Mode::Hybrid { prefilter }),
            _ => bail!("unknown mode {m}"),
        });
    }
    if !files.is_empty() {
        for path in files {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let cases = bench::parse(&text).map_err(anyhow::Error::msg)?;
            let name = path.file_stem().map_or_else(
                || path.display().to_string(),
                |s| s.to_string_lossy().into_owned(),
            );
            options.sets.push((name, cases));
        }
    } else {
        for id in sets.split(',') {
            let set = TEST_SETS
                .iter()
                .find(|s| s.id == id)
                .with_context(|| format!("unknown test set {id}"))?;
            options.sets.push((id.to_owned(), set.cases()));
        }
    }
    if options.models.is_empty() {
        options.models = installed(&options.dir)?;
    }
    Ok(options)
}

/// Returns the names of the directories under `dir` that hold a model,
/// sorted.
fn installed(dir: &Path) -> Result<Vec<String>> {
    let mut models = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if entry.path().join("model.safetensors").exists() {
            models.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    models.sort();
    Ok(models)
}
