// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The configuration file, `%APPDATA%\typomend\config.toml`.
//!
//! Correction rules are not stored here but in their own plain-text file; see
//! the `rules` module.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use typomend_observer::{AppMode, Settings};

use crate::i18n::Language;

/// The model that did best when typing the Traditional Chinese test set and
/// can be downloaded from the settings window; see
/// crates/typomend-scorer/README.md.
const DEFAULT_MODEL: &str = "macbert4csc-base-chinese";

/// Which engine corrects the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    /// Fixed substitutions from the rule list file.
    #[default]
    Rules,
    /// The rule list, plus a local masked language model that fixes
    /// homophone typos the rules do not cover.
    MaskedLanguageModel,
}

/// How the masked language model checks the text; see [`typomend_scorer::Mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LmMode {
    /// Every recent character masked in turn.
    #[default]
    Accurate,
    /// One pass to find suspects, a masked pass to confirm them.
    Balanced,
    /// One pass only.
    Fast,
}

impl LmMode {
    /// Every mode, in the order the settings window lists them.
    pub const ALL: [Self; 3] = [Self::Accurate, Self::Balanced, Self::Fast];
}

impl From<LmMode> for typomend_scorer::ModeKind {
    fn from(mode: LmMode) -> Self {
        match mode {
            LmMode::Accurate => Self::Masked,
            LmMode::Balanced => Self::Hybrid,
            LmMode::Fast => Self::OnePass,
        }
    }
}

/// Settings of the masked language model engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LmConfig {
    /// Directory name of the model under the models directory.
    pub model: String,
    /// How the model checks the text.
    pub mode: LmMode,
    /// How many times likelier than the typed character a homophone must be
    /// before it replaces it, when not the threshold recommended for the
    /// model and mode.
    pub custom_threshold: Option<f32>,
    /// In [`LmMode::Balanced`], the likelihood ratio from the first pass at
    /// which a character gets a second look.
    pub prefilter: f32,
    /// Score Traditional text in its Simplified form.
    pub simplify: bool,
    /// Where models are stored, when not in the default directory.
    pub models_dir: Option<PathBuf>,
}

impl Default for LmConfig {
    fn default() -> Self {
        let model = typomend_scorer::MODELS
            .iter()
            .find(|m| m.id == DEFAULT_MODEL);
        Self {
            model: DEFAULT_MODEL.into(),
            mode: LmMode::Accurate,
            custom_threshold: None,
            prefilter: 0.1,
            simplify: model.is_some_and(|m| m.simplify),
            models_dir: None,
        }
    }
}

/// Everything stored in the configuration file.
///
/// Fields missing from the file take their [`Default`] value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Language of the interface.
    pub language: Language,
    /// Which engine corrects the text.
    pub engine: Engine,
    /// Settings of the masked language model engine.
    pub lm: LmConfig,
    /// Whether correction is on.
    pub enabled: bool,
    /// Milliseconds without input before the typed text is checked.
    pub idle_ms: u64,
    /// How many characters before the caret are checked.
    pub caret_window: usize,
    /// Whether to correct in programs whose text cannot be read back.
    pub blind_mode: bool,
    /// Private mode: no log files and no correction history.
    pub private_mode: bool,
    /// Sample resource usage for the charts on the Usage page while
    /// it is open.
    pub usage_monitor: bool,
    /// Which log lines are written, as comma-separated `target=level`
    /// directives such as `typomend_observer=debug,info`. Read at startup;
    /// `TYPOMEND_LOG` takes precedence.
    pub log: Option<String>,
    /// Correction mode per program, keyed by lower-case executable name
    /// without extension. Programs set to [`AppMode::Auto`] are not listed.
    pub apps: BTreeMap<String, AppMode>,
}

impl Default for Config {
    fn default() -> Self {
        let observer = Settings::default();
        Self {
            language: Language::System,
            engine: Engine::Rules,
            lm: LmConfig::default(),
            enabled: observer.enabled,
            idle_ms: observer.idle_ms,
            caret_window: observer.caret_window,
            blind_mode: observer.blind_mode,
            private_mode: false,
            usage_monitor: false,
            log: None,
            apps: observer.app_modes,
        }
    }
}

impl Config {
    /// Returns the directory holding the configuration and rule files, or
    /// `None` when the platform has no configuration directory.
    pub fn dir() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("typomend"))
    }

    fn path() -> Option<PathBuf> {
        Self::dir().map(|d| d.join("config.toml"))
    }

    /// Loads the configuration, falling back to defaults when the file is
    /// missing or unreadable so that a broken file never stops the program.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("ignoring invalid {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Writes the configuration to the file, creating its directory first.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no configuration directory or the
    /// file cannot be written.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().context("no configuration directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Returns the part of the configuration the observer uses.
    pub fn observer_settings(&self) -> Settings {
        Settings {
            enabled: self.enabled,
            idle_ms: self.idle_ms,
            caret_window: self.caret_window,
            blind_mode: self.blind_mode,
            app_modes: self.apps.clone(),
        }
    }

    /// Returns the masked language model settings when that engine is chosen.
    pub fn wanted_lm(&self) -> Option<LmConfig> {
        (self.engine == Engine::MaskedLanguageModel).then(|| self.lm.clone())
    }

    /// Returns the correction mode of the program `process`, ignoring case.
    /// A program that is not listed is in [`AppMode::Auto`].
    pub fn app_mode(&self, process: &str) -> AppMode {
        self.apps
            .get(&process.to_ascii_lowercase())
            .copied()
            .unwrap_or_default()
    }

    /// Sets the correction mode of the program `process`, ignoring case.
    /// Setting [`AppMode::Auto`] removes the entry, since it is the default.
    pub fn set_app_mode(&mut self, process: &str, mode: AppMode) {
        let key = process.to_ascii_lowercase();
        if mode == AppMode::Auto {
            self.apps.remove(&key);
        } else {
            self.apps.insert(key, mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut config = Config {
            language: Language::TraditionalChinese,
            ..Config::default()
        };
        config.set_app_mode("Code", AppMode::Off);
        config.set_app_mode("WindowsTerminal", AppMode::Terminal);
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let config: Config = toml::from_str("enabled = false").unwrap();
        assert!(!config.enabled);
        assert_eq!(config.engine, Engine::Rules);
    }

    #[test]
    fn sets_app_modes_case_insensitively() {
        let mut config = Config::default();
        config.set_app_mode("Chrome", AppMode::Off);
        assert_eq!(config.app_mode("chrome"), AppMode::Off);
        config.set_app_mode("CHROME", AppMode::Auto);
        assert_eq!(config.app_mode("chrome"), AppMode::Auto);
        assert!(config.apps.is_empty());
    }

    #[test]
    fn writes_app_modes_as_a_table() {
        let mut config = Config::default();
        config.set_app_mode("WindowsTerminal", AppMode::Terminal);
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains(
            "[apps]
windowsterminal = \"terminal\""
        ));
    }
}
