// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The masked language model engine: downloading, finding and loading the chosen
//! model, and combining it with the rule list.
//!
//! Models are not bundled, since each is several hundred megabytes. The
//! settings window downloads them from Hugging Face into one directory per
//! model, named after its [`ModelInfo::id`], under the models directory: the
//! one the user chose, or [`default_models_dir`]. Loading takes about a
//! second, so it happens on a background thread; until it is done, and
//! whenever it fails, the rule list keeps correcting on its own.
//!
//! Downloads, benchmarks and checks from the Try page also run on background
//! threads. Each reports its progress through [`Engines`] and posts
//! [`app::WM_APP_REFRESH`] so the settings window shows it.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use typomend_engine::{Correction, Corrector, RuleCorrector};
use typomend_observer::{MAX_FIXES_PER_EDIT, Shared};
use typomend_scorer::bench::{self, Outcome, TEST_SETS};
use typomend_scorer::download::{self, Progress};
use typomend_scorer::{
    CandidateScorer, Detector, Homophones, LmCorrector, MODELS, Mode, ModelInfo, Script, Verdict,
};

use crate::config::{LmConfig, LmMode};
use crate::i18n;
use crate::{app, monitor};

/// Threshold for a model missing from [`MODELS`], such as one installed by
/// hand under another name.
const DEFAULT_THRESHOLD: f32 = 300.0;

/// Returns the directory models are stored in by default:
/// `D:\Typomend\models`, or `C:\Typomend\models` on a machine without a D:
/// drive. Models take hundreds of megabytes each, which a second drive
/// usually has more room for than the system drive.
pub fn default_models_dir() -> Option<PathBuf> {
    // An empty optical drive is not a directory either, so D: counts as
    // missing and the path falls back to C:.
    let drive = if Path::new(r"D:\").is_dir() { "D" } else { "C" };
    Some(PathBuf::from(format!(r"{drive}:\Typomend\models")))
}

/// Returns the directory holding the downloaded models: the one chosen in
/// `config`, or the default.
pub fn models_dir(config: &LmConfig) -> Option<PathBuf> {
    config.models_dir.clone().or_else(default_models_dir)
}

/// Returns where the model `id` is stored.
pub fn model_dir(config: &LmConfig, id: &str) -> Option<PathBuf> {
    models_dir(config).map(|d| d.join(id))
}

/// Returns whether the model `id` has been downloaded. The weights get their
/// final name only once complete, after the other files, so their presence
/// means the download finished.
pub fn is_installed(config: &LmConfig, id: &str) -> bool {
    model_dir(config, id).is_some_and(|d| d.join("model.safetensors").is_file())
}

/// Returns the entry of [`MODELS`] for `id`, or `None` for a model installed
/// by hand.
pub fn model_info(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

/// A masked language model corrector that can also explain its verdicts.
///
/// Tests implement it to stand in for a model.
trait LmEngine: Corrector + std::fmt::Debug {
    /// Returns how the model judges each character of `text`.
    fn explain(&self, text: &str) -> Result<Vec<Verdict>, typomend_scorer::Error>;
}

impl LmEngine for LmCorrector {
    fn explain(&self, text: &str) -> Result<Vec<Verdict>, typomend_scorer::Error> {
        LmCorrector::explain(self, text)
    }
}

/// State of the masked language model engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LmStatus {
    /// The rule list corrects on its own.
    Off,
    /// The chosen model is loading in the background.
    Loading,
    /// The chosen model is loaded and correcting with the rule list.
    Ready,
    /// The chosen model has not been downloaded.
    Missing,
    /// Loading the chosen model failed with this error.
    Failed(String),
}

/// A download in progress, or how the last one ended.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadState {
    /// The model `id` is downloading. `progress` is `None` until the first
    /// report.
    Running {
        id: String,
        progress: Option<Progress>,
    },
    /// Downloading the model `id` failed with `error`.
    Failed { id: String, error: String },
}

/// A benchmark in progress, or how the last one ended.
#[derive(Debug, Clone, PartialEq)]
pub enum BenchState {
    /// The benchmark of `model` is typing the test set `set`.
    Running {
        model: String,
        set: &'static str,
        /// Pairs of the test set typed so far.
        done: usize,
        /// Pairs in the test set, 0 until the first report.
        total: usize,
    },
    /// The benchmark finished.
    Done {
        /// Settings the benchmark ran with.
        config: LmConfig,
        /// Result per test set id.
        results: Vec<(&'static str, Outcome)>,
    },
    /// Loading `model` for the benchmark failed with `error`.
    Failed { model: String, error: String },
}

/// A check from the Try page in progress, or how it ended.
#[derive(Debug, Clone, PartialEq)]
pub enum TryState {
    /// The check is running in the background.
    Running,
    /// The check finished with this result.
    Done(TryResult),
    /// Scoring the text failed with this error.
    Failed(String),
}

/// What the correctors in effect make of a piece of text.
#[derive(Debug, Clone, PartialEq)]
pub struct TryResult {
    /// The text as entered.
    pub text: String,
    /// The text after committing it all at once, as an input method commits
    /// a phrase.
    pub committed: String,
    /// The corrections made to `committed`, in the order they were made.
    pub fixes: Vec<Correction>,
    /// The text after typing it a character at a time.
    pub typed: String,
    /// How the masked language model judges each character, or `None` when no
    /// model is loaded.
    pub verdicts: Option<Vec<Verdict>>,
    /// Time the whole check took.
    pub elapsed: Duration,
}

/// The correctors in effect, and the observer they are handed to.
#[derive(Debug)]
pub struct Engines {
    shared: Arc<Shared>,
    rules: Mutex<Arc<RuleCorrector>>,
    lm: Mutex<Option<Arc<dyn LmEngine>>>,
    /// The model configuration asked for last, `None` for rules only. A
    /// load that finishes after this changed is discarded.
    wanted: Mutex<Option<LmConfig>>,
    status: Mutex<LmStatus>,
    /// Memory the loaded model added to this process, measured as it
    /// loaded. `None` without one.
    model_memory: Mutex<Option<u64>>,
    download: Mutex<Option<DownloadState>>,
    /// Set to stop the running download.
    cancel: Mutex<bool>,
    bench: Mutex<Option<BenchState>>,
    /// Set to stop the running benchmark.
    bench_cancel: Mutex<bool>,
    try_state: Mutex<Option<TryState>>,
}

impl Engines {
    /// Creates the engines with the empty rule list and no model, handing
    /// corrections to the observer through `shared`.
    pub fn new(shared: Arc<Shared>) -> Arc<Self> {
        Arc::new(Self {
            shared,
            rules: Mutex::new(Arc::new(RuleCorrector::default())),
            lm: Mutex::new(None),
            wanted: Mutex::new(None),
            status: Mutex::new(LmStatus::Off),
            model_memory: Mutex::new(None),
            download: Mutex::new(None),
            cancel: Mutex::new(false),
            bench: Mutex::new(None),
            bench_cancel: Mutex::new(false),
            try_state: Mutex::new(None),
        })
    }

    /// Returns the state of the masked language model engine.
    pub fn status(&self) -> LmStatus {
        self.status.lock().clone()
    }

    /// Returns the bytes of memory the loaded model added to this process, or
    /// `None` when no model is loaded or the measurement was unavailable.
    pub fn model_memory(&self) -> Option<u64> {
        *self.model_memory.lock()
    }

    /// Returns the running download or how the last one failed, `None` when
    /// there is neither.
    pub fn download_state(&self) -> Option<DownloadState> {
        self.download.lock().clone()
    }

    /// Returns the running benchmark or how the last one ended, `None` when
    /// there is neither.
    pub fn bench_state(&self) -> Option<BenchState> {
        self.bench.lock().clone()
    }

    /// Runs every bundled test set through the model `config` names, in
    /// the background; see [`typomend_scorer::bench`].
    ///
    /// The model in use is reused when it is the one asked for, so it is not
    /// loaded twice. Does nothing while a benchmark is already running.
    pub fn start_bench(self: &Arc<Self>, config: LmConfig) {
        let mut state = self.bench.lock();
        if matches!(*state, Some(BenchState::Running { .. })) {
            return;
        }
        *state = Some(BenchState::Running {
            model: config.model.clone(),
            set: TEST_SETS[0].id,
            done: 0,
            total: 0,
        });
        drop(state);
        *self.bench_cancel.lock() = false;
        app::post(app::WM_APP_REFRESH);

        let engines = Arc::clone(self);
        std::thread::spawn(move || {
            let loaded = (engines.wanted.lock().as_ref() == Some(&config))
                .then(|| engines.lm.lock().clone())
                .flatten();
            let corrector = match loaded {
                Some(lm) => lm,
                None => match load(&config) {
                    Ok(lm) => lm,
                    Err(e) => {
                        *engines.bench.lock() = Some(BenchState::Failed {
                            model: config.model.clone(),
                            error: e.to_string(),
                        });
                        app::post(app::WM_APP_REFRESH);
                        return;
                    }
                },
            };

            let jobs = std::thread::available_parallelism().map_or(1, NonZero::get);
            let mut results = Vec::new();
            for set in TEST_SETS {
                let cases = set.cases();
                let total = cases.len();
                let outcome =
                    bench::simulate_typing(corrector.as_ref(), &cases, jobs, &mut |done| {
                        *engines.bench.lock() = Some(BenchState::Running {
                            model: config.model.clone(),
                            set: set.id,
                            done,
                            total,
                        });
                        app::post(app::WM_APP_REFRESH);
                        !*engines.bench_cancel.lock()
                    });
                let Some(outcome) = outcome else {
                    *engines.bench.lock() = None;
                    app::post(app::WM_APP_REFRESH);
                    return;
                };
                tracing::info!(
                    model = config.model,
                    set = set.id,
                    fixed = outcome.fixed,
                    false_positives = outcome.false_positives,
                    pairs = outcome.pairs,
                    ms_per_call = outcome.mean_call().as_millis() as u64,
                    "benchmark"
                );
                results.push((set.id, outcome));
            }
            *engines.bench.lock() = Some(BenchState::Done { config, results });
            app::post(app::WM_APP_REFRESH);
        });
    }

    /// Returns the running check or the result of the last one, `None` when
    /// none has run.
    pub fn try_state(&self) -> Option<TryState> {
        self.try_state.lock().clone()
    }

    /// Runs `text` through the correctors in effect in the background, the
    /// way the observer would with a caret window of `within` characters.
    ///
    /// Does nothing while a check is already running.
    pub fn start_try(self: &Arc<Self>, text: String, within: usize) {
        let mut state = self.try_state.lock();
        if matches!(*state, Some(TryState::Running)) {
            return;
        }
        *state = Some(TryState::Running);
        drop(state);
        app::post(app::WM_APP_REFRESH);

        let corrector = Combined {
            rules: Arc::clone(&self.rules.lock()),
            lm: self.lm.lock().clone(),
        };
        let engines = Arc::clone(self);
        std::thread::spawn(move || {
            let started = Instant::now();
            let verdicts = corrector.lm.as_ref().map(|lm| lm.explain(&text));
            let state = match verdicts.transpose() {
                Ok(verdicts) => {
                    let mut committed = text.clone();
                    let mut fixes = Vec::new();
                    correct(&corrector, &mut committed, within, &mut fixes);
                    let mut typed = String::new();
                    for c in text.chars() {
                        typed.push(c);
                        correct(&corrector, &mut typed, within, &mut Vec::new());
                    }
                    TryState::Done(TryResult {
                        text,
                        committed,
                        fixes,
                        typed,
                        verdicts,
                        elapsed: started.elapsed(),
                    })
                }
                Err(e) => TryState::Failed(e.to_string()),
            };
            *engines.try_state.lock() = Some(state);
            app::post(app::WM_APP_REFRESH);
        });
    }

    /// Stops the running benchmark, discarding its results.
    pub fn cancel_bench(&self) {
        *self.bench_cancel.lock() = true;
    }

    /// Replaces the rule list and hands the combined corrector to the
    /// observer.
    pub fn set_rules(&self, rules: RuleCorrector) {
        *self.rules.lock() = Arc::new(rules);
        self.install();
    }

    /// Switches to `wanted`, loading its model in the background, or back
    /// to rules only with `None`. Does nothing if `wanted` is already in
    /// effect or loading.
    pub fn want(self: &Arc<Self>, wanted: Option<LmConfig>) {
        let mut current = self.wanted.lock();
        if *current == wanted {
            return;
        }
        *current = wanted.clone();
        drop(current);

        // Free the old model first, so two never sit in memory together.
        *self.lm.lock() = None;
        *self.model_memory.lock() = None;
        self.install();
        let Some(config) = wanted else {
            self.set_status(LmStatus::Off);
            return;
        };
        if !is_installed(&config, &config.model) {
            self.set_status(LmStatus::Missing);
            return;
        }
        self.set_status(LmStatus::Loading);

        let engines = Arc::clone(self);
        std::thread::spawn(move || {
            let before = monitor::private_bytes();
            let result = load(&config);
            let used = monitor::private_bytes()
                .zip(before)
                .map(|(after, before)| after.saturating_sub(before));
            if engines.wanted.lock().as_ref() != Some(&config) {
                return;
            }
            match result {
                Ok(lm) => {
                    tracing::info!(
                        model = config.model,
                        memory_mb = used.map(|b| b >> 20),
                        "masked language model loaded"
                    );
                    *engines.lm.lock() = Some(lm);
                    *engines.model_memory.lock() = used;
                    engines.install();
                    engines.set_status(LmStatus::Ready);
                }
                Err(e) => {
                    tracing::error!(
                        model = config.model,
                        "loading the masked language model failed: {e}"
                    );
                    engines.set_status(LmStatus::Failed(e.to_string()));
                }
            }
        });
    }

    /// Loads the wanted model again, as after it was downloaded or deleted.
    pub fn reload(self: &Arc<Self>) {
        let wanted = self.wanted.lock().take();
        self.want(wanted);
    }

    /// Downloads `model` into `dir` in the background, resuming an earlier
    /// attempt. Only one download runs at a time.
    pub fn start_download(self: &Arc<Self>, model: &'static ModelInfo, dir: PathBuf) {
        let mut download = self.download.lock();
        if matches!(*download, Some(DownloadState::Running { .. })) {
            return;
        }
        *download = Some(DownloadState::Running {
            id: model.id.into(),
            progress: None,
        });
        drop(download);
        *self.cancel.lock() = false;
        app::post(app::WM_APP_REFRESH);

        let engines = Arc::clone(self);
        std::thread::spawn(move || {
            tracing::info!(model = model.id, dir = %dir.display(), "downloading");
            let result = download::download(model, &dir, &mut |progress| {
                *engines.download.lock() = Some(DownloadState::Running {
                    id: model.id.into(),
                    progress: Some(progress),
                });
                app::post(app::WM_APP_REFRESH);
                !*engines.cancel.lock()
            });
            *engines.download.lock() = match result {
                Ok(()) => {
                    tracing::info!(model = model.id, "download complete");
                    None
                }
                Err(typomend_scorer::Error::Cancelled) => None,
                Err(e) => {
                    tracing::error!(model = model.id, "download failed: {e}");
                    Some(DownloadState::Failed {
                        id: model.id.into(),
                        error: e.to_string(),
                    })
                }
            };
            engines.reload();
            app::post(app::WM_APP_REFRESH);
        });
    }

    /// Stops the running download; it resumes when started again.
    pub fn cancel_download(&self) {
        *self.cancel.lock() = true;
    }

    /// Deletes the downloaded model in `dir`, unloading it first if it is
    /// the one in use.
    pub fn delete_model(self: &Arc<Self>, dir: &Path) -> std::io::Result<()> {
        *self.lm.lock() = None;
        *self.model_memory.lock() = None;
        self.install();
        let result = std::fs::remove_dir_all(dir);
        self.reload();
        result
    }

    /// Records `status` and asks the settings window to redraw.
    fn set_status(&self, status: LmStatus) {
        *self.status.lock() = status;
        app::post(app::WM_APP_REFRESH);
    }

    /// Hands the rule list and the loaded model, if any, to the observer.
    fn install(&self) {
        let combined = Combined {
            rules: Arc::clone(&self.rules.lock()),
            lm: self.lm.lock().clone(),
        };
        self.shared.set_corrector(Arc::new(combined));
    }
}

/// Loads the model `config` names and wraps it in a corrector.
///
/// # Errors
///
/// Returns an error when the model files are missing or cannot be loaded.
fn load(config: &LmConfig) -> Result<Arc<dyn LmEngine>, typomend_scorer::Error> {
    let dir = model_dir(config, &config.model).unwrap_or_default();
    let scorer = CandidateScorer::load(dir, config.simplify)?;
    let mode = match config.mode {
        LmMode::Accurate => Mode::Masked,
        LmMode::Balanced => Mode::Hybrid {
            prefilter: config.prefilter,
        },
        LmMode::Fast => Mode::OnePass,
    };
    let homophones = Homophones::builtin().assuming(users_script());
    let detector = Detector::new(scorer, homophones, mode);
    Ok(Arc::new(LmCorrector::new(detector, threshold(config))))
}

/// Guesses the script the user writes in from the Windows locale. Chinese
/// locales without a Traditional marker mean Simplified; Traditional Chinese
/// is also the guess for every other locale.
fn users_script() -> Script {
    let locale = sys_locale::get_locale().unwrap_or_default();
    if i18n::locale_writes_simplified(&locale) {
        Script::Simplified
    } else {
        Script::Traditional
    }
}

/// Returns the threshold `config` asks for: its own, or the one
/// recommended for its model and mode.
pub fn threshold(config: &LmConfig) -> f32 {
    config
        .custom_threshold
        .or_else(|| model_info(&config.model).map(|m| m.threshold(config.mode.into())))
        .unwrap_or(DEFAULT_THRESHOLD)
}

/// Applies the corrections `corrector` finds in `text`, appending them to
/// `fixes`, as the observer does after the user commits text.
///
/// Stops after [`MAX_FIXES_PER_EDIT`] fixes, or at a correction that does
/// not match the text it claims to replace.
fn correct(
    corrector: &dyn Corrector,
    text: &mut String,
    within: usize,
    fixes: &mut Vec<Correction>,
) {
    for _ in 0..MAX_FIXES_PER_EDIT {
        let Some(fix) = corrector.last_correction(text, within) else {
            return;
        };
        if text.get(fix.start..fix.end()) != Some(fix.from.as_str()) {
            return;
        }
        text.replace_range(fix.start..fix.end(), &fix.to);
        fixes.push(fix);
    }
}

/// The rule list and, when loaded, the masked language model. The rule list is the
/// user's own word, so it wins when both find something at the same place.
#[derive(Debug)]
struct Combined {
    rules: Arc<RuleCorrector>,
    lm: Option<Arc<dyn LmEngine>>,
}

impl Corrector for Combined {
    fn last_correction(&self, text: &str, within: usize) -> Option<Correction> {
        let rule = self.rules.last_correction(text, within);
        let Some(lm) = &self.lm else {
            return rule;
        };
        match (rule, lm.last_correction(text, within)) {
            (Some(r), Some(l)) if l.end() > r.end() => Some(l),
            (Some(r), _) => Some(r),
            (None, l) => l,
        }
    }
}

#[cfg(test)]
mod tests {
    use typomend_engine::Rule;

    use super::*;

    /// Always suggests one fixed correction.
    #[derive(Debug)]
    struct Fixed(Option<Correction>);

    impl Corrector for Fixed {
        fn last_correction(&self, _text: &str, _within: usize) -> Option<Correction> {
            self.0.clone()
        }
    }

    impl LmEngine for Fixed {
        fn explain(&self, _text: &str) -> Result<Vec<Verdict>, typomend_scorer::Error> {
            Ok(Vec::new())
        }
    }

    /// Builds a correction of `from` to `to` at byte offset `start`.
    fn correction(start: usize, from: &str, to: &str) -> Correction {
        Correction {
            start,
            from: from.into(),
            to: to.into(),
        }
    }

    /// Builds a [`Combined`] with one rule and a model that suggests `lm`.
    fn combined(lm: Option<Correction>) -> Combined {
        Combined {
            rules: Arc::new(RuleCorrector::new(vec![Rule::new("因該", "應該")])),
            lm: Some(Arc::new(Fixed(lm))),
        }
    }

    #[test]
    fn uses_the_rules_without_a_model() {
        let rules_only = Combined {
            rules: Arc::new(RuleCorrector::new(vec![Rule::new("因該", "應該")])),
            lm: None,
        };
        let fix = rules_only.last_correction("這個因該可以", 32).unwrap();
        assert_eq!(fix.to, "應該");
    }

    #[test]
    fn uses_the_model_where_the_rules_find_nothing() {
        let lm = correction(15, "求", "球");
        let fix = combined(Some(lm.clone())).last_correction("我今天去打籃求", 32);
        assert_eq!(fix, Some(lm));
    }

    #[test]
    fn prefers_the_rules_unless_the_model_fix_is_later() {
        // "這個因該可以，打籃求": the rule ends at byte 12, the model fix later.
        let text = "這個因該可以，打籃求";
        let later = correction(text.len() - 3, "求", "球");
        assert_eq!(
            combined(Some(later.clone())).last_correction(text, 32),
            Some(later)
        );
        let earlier = correction(0, "這", "者");
        let fix = combined(Some(earlier)).last_correction(text, 32).unwrap();
        assert_eq!(fix.to, "應該");
    }

    #[test]
    fn follows_the_recommended_threshold_unless_overridden() {
        let mut config = LmConfig {
            model: "macbert4csc-base-chinese".into(),
            mode: LmMode::Balanced,
            ..LmConfig::default()
        };
        let recommended = model_info(&config.model)
            .unwrap()
            .threshold(LmMode::Balanced.into());
        assert_eq!(threshold(&config), recommended);
        config.custom_threshold = Some(5.0);
        assert_eq!(threshold(&config), 5.0);
        config.model = "hand-installed".into();
        config.custom_threshold = None;
        assert_eq!(threshold(&config), DEFAULT_THRESHOLD);
    }

    #[test]
    fn stores_models_on_d_when_there_is_one() {
        let dir = default_models_dir().unwrap();
        let drive = if Path::new(r"D:\").is_dir() { 'D' } else { 'C' };
        assert_eq!(dir, PathBuf::from(format!(r"{drive}:\Typomend\models")));
    }

    #[test]
    fn a_chosen_folder_overrides_the_default() {
        let config = LmConfig {
            models_dir: Some(PathBuf::from(r"E:\models")),
            ..LmConfig::default()
        };
        assert_eq!(models_dir(&config), Some(PathBuf::from(r"E:\models")));
        assert_eq!(
            model_dir(&config, "google-bert-base-chinese"),
            Some(PathBuf::from(r"E:\models\google-bert-base-chinese"))
        );
    }
}
