// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Settings the user can change while the observer runs, and the state the
//! observer thread shares with the rest of the program.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use typomend_engine::Corrector;

/// How the user wants one program to be corrected. Password fields and
/// elevated programs are left alone whatever the mode says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppMode {
    /// Decide from what the focused control exposes. Terminals get terminal
    /// mode, and controls that hide their text are corrected blindly only
    /// when blind mode is allowed.
    #[default]
    Auto,
    /// Only controls whose text is readable, corrected by selecting the typo.
    /// Terminals, where selecting does not move the cursor, are left alone.
    Standard,
    /// Arrow keys relative to the caret, even where the text is readable,
    /// and whether or not blind mode is allowed in general.
    Blind,
    /// Text read through UIA, typo replaced with arrow keys and Backspace,
    /// result read back. Suits command lines, where selecting text does not
    /// move the input cursor.
    Terminal,
    /// Never correct this program.
    Off,
}

impl AppMode {
    /// Every mode, in the order the settings window lists them.
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Standard,
        Self::Blind,
        Self::Terminal,
        Self::Off,
    ];
}

/// Observer behavior that the user can change at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Whether the observer corrects anything at all.
    pub enabled: bool,
    /// How long the keyboard must be idle before a correction is typed, in
    /// milliseconds. Typed corrections interleave with keys the user presses
    /// at the same time, so this must stay above zero.
    pub idle_ms: u64,
    /// Only typos ending within this many UTF-16 units before the caret are
    /// treated as just typed; anything further away is left alone.
    pub caret_window: usize,
    /// Whether controls that hide their text may be corrected blindly.
    pub blind_mode: bool,
    /// Modes chosen per program, keyed by lower-case executable name without
    /// extension. Programs not listed use [`AppMode::Auto`].
    pub app_modes: BTreeMap<String, AppMode>,
}

impl Settings {
    /// Returns the mode chosen for `process`, ignoring case.
    pub fn app_mode(&self, process: &str) -> AppMode {
        self.app_modes
            .get(&process.to_ascii_lowercase())
            .copied()
            .unwrap_or_default()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_ms: 300,
            caret_window: 32,
            blind_mode: true,
            app_modes: BTreeMap::new(),
        }
    }
}

/// State shared between the observer thread and the rest of the program.
pub struct Shared {
    settings: RwLock<Settings>,
    corrector: RwLock<Arc<dyn Corrector>>,
    /// Bumped on every settings change, so the observer can tell that what
    /// it decided about the focused control may be out of date.
    generation: AtomicU64,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("settings", &*self.settings.read())
            .finish_non_exhaustive()
    }
}

impl Shared {
    /// Creates the state to hand to an [`Observer`](crate::Observer).
    pub fn new(settings: Settings, corrector: Arc<dyn Corrector>) -> Arc<Self> {
        Arc::new(Self {
            settings: RwLock::new(settings),
            corrector: RwLock::new(corrector),
            generation: AtomicU64::new(0),
        })
    }

    /// Returns a copy of the current settings.
    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    /// Applies `f` to the settings. The observer re-evaluates the focused
    /// control on its next tick, so a program that was just disabled stops
    /// being corrected immediately.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings.write());
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Returns the corrector in use.
    pub fn corrector(&self) -> Arc<dyn Corrector> {
        self.corrector.read().clone()
    }

    /// Replaces the corrector in use. Corrections from then on use
    /// `corrector`.
    pub fn set_corrector(&self, corrector: Arc<dyn Corrector>) {
        *self.corrector.write() = corrector;
    }

    /// Returns a counter that changes whenever the settings do.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}
