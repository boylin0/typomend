// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! State shared by the settings window, the tray icon and the observer
//! callback.
//!
//! All of it lives in one [`AppState`] behind an `Arc`. Each part locks only
//! the field it touches, and changes reach the settings window as a posted
//! [`app::WM_APP_REFRESH`], so only the UI thread updates controls.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use typomend_engine::RuleCorrector;
use typomend_observer::{AppMode, CorrectionRecord, Event, Shared, Support};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::app;
use crate::config::Config;
use crate::lm::Engines;
use crate::logging;
use crate::monitor::Monitor;
use crate::rules::{self, RuleList};

/// Most corrections kept for the History page.
const HISTORY_LIMIT: usize = 100;

/// A correction as listed on the History page.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Local time as `HH:MM:SS`.
    pub time: String,
    pub record: CorrectionRecord,
}

/// What the observer has seen since start.
#[derive(Debug, Default)]
pub struct Status {
    /// The program in focus and how it is handled.
    pub current: Option<(String, Support)>,
    /// How each program was last handled, and the mode it had then.
    pub apps: BTreeMap<String, (Support, AppMode)>,
    /// Recent corrections, newest first, at most [`HISTORY_LIMIT`]. Empty in
    /// private mode.
    pub history: VecDeque<HistoryEntry>,
    /// Corrections made since start, private mode included.
    pub count: usize,
}

/// Everything the settings window, the tray icon and the observer share.
#[derive(Debug)]
pub struct AppState {
    /// The configuration. Read it through [`AppState::config`] and change it
    /// through [`AppState::update_config`], which keep the observer and the
    /// file on disk in step.
    config: Mutex<Config>,
    /// Settings and corrector handed to the observer thread.
    pub shared: Arc<Shared>,
    pub status: Mutex<Status>,
    /// The rule list as last loaded by [`AppState::reload_rules`].
    pub rules: Mutex<RuleList>,
    /// The rules and the masked language model that produce corrections for the
    /// observer.
    pub engines: Arc<Engines>,
    /// Resource usage of this process, sampled while the Usage page is
    /// open.
    pub monitor: Arc<Monitor>,
}

impl AppState {
    /// Creates the state for `config` and starts loading the masked language model
    /// it asks for.
    ///
    /// No rules are in effect until [`reload_rules`](Self::reload_rules)
    /// runs.
    pub fn new(config: Config) -> Arc<Self> {
        let shared = Shared::new(
            config.observer_settings(),
            Arc::new(RuleCorrector::default()),
        );
        let engines = Engines::new(Arc::clone(&shared));
        engines.want(config.wanted_lm());
        Arc::new(Self {
            config: Mutex::new(config),
            engines,
            monitor: Monitor::new(),
            shared,
            status: Mutex::default(),
            rules: Mutex::default(),
        })
    }

    /// Returns a copy of the configuration, so no lock is held while it is
    /// used.
    pub fn config(&self) -> Config {
        self.config.lock().clone()
    }

    /// Applies `f` to the configuration, pushes the result to the observer,
    /// saves it and refreshes the settings window. Nothing happens if `f`
    /// changes nothing.
    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut config = self.config.lock();
        let before = config.clone();
        f(&mut config);
        if *config == before {
            return;
        }
        if config.wanted_lm() != before.wanted_lm() {
            self.engines.want(config.wanted_lm());
        }
        if config.observer_settings() != before.observer_settings() {
            let settings = config.observer_settings();
            self.shared.update_settings(|s| *s = settings);
        }
        // Log the switch while logging is on, before pausing it or after
        // resuming it, so the file shows where the gap is.
        if config.private_mode && !before.private_mode {
            tracing::info!("private mode on");
            logging::set_private(true);
        } else if !config.private_mode && before.private_mode {
            logging::set_private(false);
            tracing::info!("private mode off");
        }
        if config.private_mode {
            self.status.lock().history.clear();
        }
        if let Err(e) = config.save() {
            tracing::error!("saving the configuration failed: {e:#}");
        }
        drop(config);
        app::post(app::WM_APP_REFRESH);
    }

    /// Reads the rule file again and puts the new rules into effect. May be
    /// called from any thread.
    pub fn reload_rules(&self) {
        let list = rules::load();
        let corrector = RuleCorrector::new(list.rules.to_vec());
        tracing::info!(
            rules = corrector.len(),
            errors = list.errors.len(),
            missing = list.missing,
            "rules loaded"
        );
        self.engines.set_rules(corrector);
        *self.rules.lock() = list;
        app::post(app::WM_APP_REFRESH);
    }

    /// Asks the UI thread to open the settings window. May be called from
    /// any thread.
    pub fn show_window(&self) {
        app::post(app::WM_APP_SHOW);
    }

    /// Records an observer event and asks the settings window to refresh.
    ///
    /// Called on the observer thread. In private mode a correction is counted
    /// but not listed in the history.
    pub fn on_event(&self, event: Event) {
        let config = self.config();
        let keep_history = !config.private_mode;
        let mut status = self.status.lock();
        match event {
            Event::Focus { process, support } => {
                if !process.is_empty() {
                    let mode = config.app_mode(&process);
                    status.apps.insert(process.clone(), (support, mode));
                }
                status.current = Some((process, support));
            }
            Event::Correction(record) => {
                status.count += 1;
                if keep_history {
                    status.history.push_front(HistoryEntry {
                        time: local_time(),
                        record,
                    });
                    status.history.truncate(HISTORY_LIMIT);
                }
            }
        }
        drop(status);
        app::post(app::WM_APP_REFRESH);
    }
}

/// Returns the local date as `YYYY-MM-DD`.
pub(crate) fn local_date() -> String {
    // SAFETY: GetLocalTime takes no arguments and returns the time by value.
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// Returns the local time as `HH:MM:SS`.
pub(crate) fn local_time() -> String {
    // SAFETY: GetLocalTime takes no arguments and returns the time by value.
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}
