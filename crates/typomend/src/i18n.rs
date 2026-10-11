// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Localization through Fluent.
//!
//! Translations live in `i18n/<language>/typomend.ftl` and are embedded in the
//! binary. The [`fl!`](crate::fl) macro checks message ids at compile time.

use std::sync::LazyLock;

use i18n_embed::LanguageLoader;
use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

/// The embedded translation files.
#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

/// The Fluent loader that [`fl!`](crate::fl) looks messages up in.
///
/// It starts with the fallback language; [`apply`] selects the user's.
pub static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    loader
        .load_fallback_language(&Localizations)
        .expect("the fallback language is embedded");
    // Fluent wraps placeables in Unicode isolation marks by default, which
    // Win32 static controls draw as visible boxes.
    loader.set_use_isolating(false);
    loader
});

/// Looks up a localized message, checking the id at compile time.
#[macro_export]
macro_rules! fl {
    ($id:literal) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $id)
    };
    ($id:literal, $($args:tt)*) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $id, $($args)*)
    };
}

/// The user's language choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    /// Follow the language of the system.
    #[default]
    System,
    /// English.
    #[serde(rename = "en")]
    English,
    /// Traditional Chinese.
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
}

impl Language {
    /// Explicit choices, in the order shown in the settings.
    pub const CHOICES: [Self; 2] = [Self::English, Self::TraditionalChinese];

    /// Returns the language name in that language, which is how users look
    /// for their own language in a list.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::System => "",
            Self::English => "English",
            Self::TraditionalChinese => "繁體中文",
        }
    }

    /// Resolves [`Language::System`] to a concrete language.
    ///
    /// A system locale that cannot be read resolves to English. Any other
    /// choice is returned unchanged.
    pub fn resolve(self) -> Self {
        match self {
            Self::System => from_locale(&sys_locale::get_locale().unwrap_or_default()),
            other => other,
        }
    }

    /// Returns the tag of the translation files for the resolved language.
    fn tag(self) -> &'static str {
        match self.resolve() {
            Self::TraditionalChinese => "zh-TW",
            _ => "en",
        }
    }
}

/// Returns whether `locale` is a Chinese locale.
fn is_chinese(locale: &str) -> bool {
    locale.to_ascii_lowercase().starts_with("zh")
}

/// Returns whether `locale` is a Chinese locale that writes Traditional
/// characters, as `zh-TW`, `zh-HK`, `zh-MO` and `zh-Hant-*` do.
fn is_traditional_chinese(locale: &str) -> bool {
    let lower = locale.to_ascii_lowercase();
    lower.starts_with("zh")
        && ["hant", "-tw", "-hk", "-mo"]
            .iter()
            .any(|m| lower.contains(m))
}

/// Returns whether the Windows locale is a Chinese one without a Traditional
/// marker, such as `zh-CN`, `zh-SG` or `zh-Hans`, so its user most likely
/// writes Simplified Chinese.
pub fn locale_writes_simplified(locale: &str) -> bool {
    is_chinese(locale) && !is_traditional_chinese(locale)
}

/// Maps a BCP 47 locale to a supported interface language.
///
/// Done by hand rather than through language negotiation because Windows
/// reports Traditional Chinese as `zh-TW`, `zh-HK` or `zh-Hant-*`, and generic
/// negotiation may match any of them to another `zh` locale on the shared
/// prefix. Every other locale gets English.
fn from_locale(locale: &str) -> Language {
    if is_traditional_chinese(locale) {
        Language::TraditionalChinese
    } else {
        Language::English
    }
}

/// Returns the language the messages are in, as chosen with [`apply`].
#[cfg(test)]
fn current() -> Language {
    match LOADER.current_language().to_string().as_str() {
        "zh-TW" => Language::TraditionalChinese,
        _ => Language::English,
    }
}

/// Switches every message to `language`, resolving [`Language::System`] first.
///
/// A failure to load the translation is logged and leaves the previous
/// language in place.
pub fn apply(language: Language) {
    let id: LanguageIdentifier = language.tag().parse().expect("valid language tag");
    if let Err(e) = i18n_embed::select(&*LOADER, &Localizations, &[id]) {
        tracing::warn!("selecting language {} failed: {e}", language.tag());
    }
    // Selecting a language rebuilds the Fluent bundles, which brings the
    // isolation marks back.
    LOADER.set_use_isolating(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_windows_locales() {
        assert_eq!(from_locale("zh-TW"), Language::TraditionalChinese);
        assert_eq!(from_locale("zh-Hant-HK"), Language::TraditionalChinese);
        assert_eq!(from_locale("zh-MO"), Language::TraditionalChinese);
        assert_eq!(from_locale("zh-CN"), Language::English);
        assert_eq!(from_locale("zh-Hans-SG"), Language::English);
        assert_eq!(from_locale("en-US"), Language::English);
        assert_eq!(from_locale(""), Language::English);
    }

    #[test]
    fn guesses_the_script_of_the_locale() {
        for locale in ["zh-CN", "zh-SG", "zh-Hans", "zh-Hans-CN", "ZH-cn", "zh"] {
            assert!(locale_writes_simplified(locale), "{locale}");
        }
        for locale in [
            "zh-TW",
            "zh-HK",
            "zh-MO",
            "zh-Hant",
            "zh-Hant-TW",
            "en-US",
            "",
        ] {
            assert!(!locale_writes_simplified(locale), "{locale}");
        }
    }

    /// Returns the sorted message ids defined in a Fluent file.
    fn message_ids(source: &str) -> Vec<&str> {
        let mut ids: Vec<&str> = source
            .lines()
            .filter(|line| line.starts_with(|c: char| c.is_ascii_alphabetic()))
            .filter_map(|line| line.split_once('=').map(|(id, _)| id.trim()))
            .collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn every_language_has_the_same_messages() {
        let en = include_str!("../i18n/en/typomend.ftl");
        let zh_tw = include_str!("../i18n/zh-TW/typomend.ftl");
        assert_eq!(message_ids(en), message_ids(zh_tw));
    }

    #[test]
    fn applying_a_language_round_trips() {
        for language in Language::CHOICES {
            apply(language);
            assert_eq!(current(), language);
            assert!(!fl!("menu-quit").is_empty());
        }
    }
}
