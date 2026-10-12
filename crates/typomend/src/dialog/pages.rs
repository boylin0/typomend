// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Behavior of each property page.
//!
//! Every page has the same entry points, called by the dialog procedure in the
//! parent module:
//!
//! * `init` once, to set up controls that need more than the template.
//! * `texts` to fill in captions, again whenever the language changes.
//! * `refresh` to show current state. `force` redraws everything; otherwise
//!   lists are rebuilt only when their contents changed, so a refresh never
//!   resets the user's scroll position or selection for nothing.
//! * `command` and `notify` for user input.
//! * `display_info`, `draw_item` and `destroy` for the list view, owner-drawn
//!   controls and teardown.
//!
//! Pages keep no copy of the settings. Each control change is written to the
//! shared [`AppState`](crate::state::AppState) at once, and each refresh reads
//! it back, so the window never disagrees with the configuration.
//!
//! The Masked language model page lists only models that download as safetensors,
//! plus the chosen model when it is not one of them.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use typomend_engine::ParseErrorKind;
use typomend_observer::{AppMode, ExcludeReason, Outcome, Support};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, FW_BOLD, GetObjectW, HFONT, HGDIOBJ, InvalidateRect,
    LOGFONTW,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, NMHDR, NMLISTVIEW, NMLVDISPINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, HICON, MapDialogRect, STM_SETICON, WM_SETFONT,
};

use super::chart::{self, Chart, Series};
use super::controls::{self, LVIS_SELECTED, ListView};
use super::ids::*;
use super::{Page, state};
use crate::config::{Config, Engine, LmMode};
use crate::i18n::Language;
use typomend_scorer::Measured;
use typomend_scorer::bench::TEST_SETS;

use crate::lm::{self, BenchState, DownloadState, LmStatus};
use crate::monitor::{self, Usage};
use crate::rules::{self, RuleList};
use crate::{fl, icon, logging, platform, tray};

const BN_CLICKED: u32 = 0;
const EN_CHANGE: u32 = 0x0300;
const CBN_SELCHANGE: u32 = 1;
const LVN_ITEMCHANGED: u32 = -101i32 as u32;
const NM_CLICK: u32 = -2i32 as u32;
const NM_RETURN: u32 = -4i32 as u32;
const LVIF_TEXT: u32 = 0x0001;
const LVIF_STATE: u32 = 0x0008;
const WM_GETFONT: u32 = 0x0031;

// Bounds of the two spin controls on the Settings page.
const IDLE_RANGE: (i32, i32) = (100, 2000);
const LICENSE_URL: &str = "https://mozilla.org/MPL/2.0/";
const CARET_RANGE: (i32, i32) = (4, 256);

thread_local! {
    // Content fingerprints of the lists currently shown, per page.
    static SHOWN: RefCell<HashMap<Page, u64>> = RefCell::new(HashMap::new());
    // GDI font of the program name on the About page, deleted with the page.
    static TITLE_FONT: RefCell<Option<HFONT>> = const { RefCell::new(None) };
    // Icon on the About page, destroyed with the page.
    static ABOUT_LOGO: RefCell<Option<HICON>> = const { RefCell::new(None) };
    // Model id of each entry in the model box, in order.
    static MODEL_IDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Returns whether `content` differs from what `page` shows, or `force` is
/// set, and records `content` as shown.
///
/// Comparing hashes spares each page a copy of what it displays.
fn changed(page: Page, content: impl Hash, force: bool) -> bool {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let hash = hasher.finish();
    SHOWN.with(|s| s.borrow_mut().insert(page, hash)) != Some(hash) || force
}

/// Sets up the controls of `page` that need more than its template, once,
/// when the page is created.
pub(super) fn init(page: Page, hwnd: HWND) {
    match page {
        Page::Rules => {
            ListView::new(hwnd, IDC_RULES).setup(&[(fl!("rules-from"), 1), (fl!("rules-to"), 1)]);
        }
        Page::Apps => {
            ListView::new(hwnd, IDC_APPS).setup(&[
                (fl!("apps-name"), 2),
                (fl!("apps-mode"), 2),
                (fl!("apps-support"), 3),
            ]);
        }
        Page::History => {
            ListView::new(hwnd, IDC_HISTORY).setup(&[
                (fl!("history-time"), 2),
                (fl!("history-app"), 2),
                (fl!("history-change"), 3),
                (fl!("history-result"), 3),
            ]);
        }
        Page::Settings => {
            let config = state().config();
            controls::spin_setup(
                hwnd,
                IDC_IDLE_SPIN,
                IDLE_RANGE.0,
                IDLE_RANGE.1,
                config.idle_ms as i32,
            );
            controls::spin_setup(
                hwnd,
                IDC_CARET_SPIN,
                CARET_RANGE.0,
                CARET_RANGE.1,
                config.caret_window as i32,
            );
        }
        Page::About => {
            set_title_font(hwnd);
            set_logo(hwnd);
        }
        Page::Overview | Page::Model | Page::Usage => {}
    }
}

/// Fills in every caption of `page` in the current language.
///
/// Also rebuilds the combo boxes whose entries are translated, which resets
/// their selection, so callers follow it with a forced [`refresh`].
pub(super) fn texts(page: Page, hwnd: HWND) {
    match page {
        Page::Overview => {
            controls::set_text(hwnd, IDC_ENABLED, &fl!("overview-enabled"));
            controls::set_text(hwnd, IDC_FOCUS_LABEL, &fl!("overview-focus"));
        }
        Page::Rules => {
            controls::set_text(hwnd, IDC_ENGINE_LABEL, &fl!("rules-engine"));
            controls::set_text(hwnd, IDC_ENGINE_RULES, &fl!("rules-engine-rules"));
            controls::set_text(hwnd, IDC_ENGINE_MLM, &fl!("rules-engine-mlm"));
            controls::set_text(hwnd, IDC_EDIT_RULES, &fl!("rules-edit"));
            controls::set_text(hwnd, IDC_OPEN_RULES_FOLDER, &fl!("rules-open-folder"));
            let path = rules::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            controls::set_text(hwnd, IDC_RULES_FILE, &fl!("rules-file", path = path));
            ListView::new(hwnd, IDC_RULES).set_column_titles(&[fl!("rules-from"), fl!("rules-to")]);
        }
        Page::Model => {
            controls::set_text(hwnd, IDC_LM_INTRO, &fl!("lm-intro"));
            controls::set_text(hwnd, IDC_LM_MODEL_LABEL, &fl!("lm-model"));
            controls::set_text(hwnd, IDC_LM_MODE_LABEL, &fl!("lm-mode"));
            controls::set_text(hwnd, IDC_LM_DELETE, &fl!("lm-delete"));
            controls::set_text(hwnd, IDC_LM_BENCH, &fl!("lm-bench"));
            controls::set_text(hwnd, IDC_LM_CHANGE_DIR, &fl!("lm-change-dir"));
            controls::set_text(hwnd, IDC_OPEN_MODELS_FOLDER, &fl!("lm-open-folder"));
            controls::set_text(hwnd, IDC_LM_TRY, &fl!("lm-try"));
            controls::set_text(hwnd, IDC_LM_NOTE, &fl!("lm-note"));
            let modes: Vec<String> = LmMode::ALL.iter().map(|&m| lm_mode_label(m)).collect();
            controls::combo_fill(hwnd, IDC_LM_MODE, &modes, 0);
            fill_models(hwnd);
            controls::combo_widen_list(hwnd, IDC_LM_MODEL, 160);
        }
        Page::Usage => {
            controls::set_text(hwnd, IDC_USAGE_ENABLED, &fl!("usage-enabled"));
            controls::set_text(hwnd, IDC_USAGE_HINT, &fl!("usage-hint"));
        }
        Page::Apps => {
            controls::set_text(hwnd, IDC_APPS_INTRO, &fl!("apps-intro"));
            controls::set_text(hwnd, IDC_APP_MODE_LABEL, &fl!("apps-mode-label"));
            ListView::new(hwnd, IDC_APPS).set_column_titles(&[
                fl!("apps-name"),
                fl!("apps-mode"),
                fl!("apps-support"),
            ]);
            let modes: Vec<String> = AppMode::ALL.iter().map(|&m| mode_label(m)).collect();
            controls::combo_fill(hwnd, IDC_APP_MODE, &modes, 0);
        }
        Page::History => {
            controls::set_text(hwnd, IDC_HISTORY_INTRO, &fl!("history-intro"));
            controls::set_text(hwnd, IDC_CLEAR, &fl!("history-clear"));
            ListView::new(hwnd, IDC_HISTORY).set_column_titles(&[
                fl!("history-time"),
                fl!("history-app"),
                fl!("history-change"),
                fl!("history-result"),
            ]);
        }
        Page::Settings => {
            controls::set_text(hwnd, IDC_LANGUAGE_LABEL, &fl!("settings-language"));
            controls::set_text(hwnd, IDC_AUTOSTART, &fl!("settings-autostart"));
            controls::set_text(hwnd, IDC_PRIVATE, &fl!("settings-private"));
            controls::set_text(hwnd, IDC_PRIVATE_HINT, &fl!("settings-private-hint"));
            controls::set_text(hwnd, IDC_IDLE_LABEL, &fl!("settings-idle"));
            controls::set_text(hwnd, IDC_IDLE_HINT, &fl!("settings-idle-hint"));
            controls::set_text(hwnd, IDC_CARET_LABEL, &fl!("settings-caret-window"));
            controls::set_text(hwnd, IDC_BLIND, &fl!("settings-blind"));
            controls::set_text(hwnd, IDC_BLIND_HINT, &fl!("settings-blind-hint"));
            controls::set_text(hwnd, IDC_OPEN_LOGS, &fl!("settings-open-logs"));
            controls::set_text(hwnd, IDC_DELETE_LOGS, &fl!("settings-delete-logs"));
            let mut languages = vec![fl!("settings-language-system")];
            languages.extend(
                Language::CHOICES
                    .iter()
                    .map(|l| l.native_name().to_string()),
            );
            controls::combo_fill(
                hwnd,
                IDC_LANGUAGE,
                &languages,
                language_index(state().config().language),
            );
        }
        Page::About => {
            controls::set_text(
                hwnd,
                IDC_ABOUT_VERSION,
                &fl!("about-version", version = env!("CARGO_PKG_VERSION")),
            );
            controls::set_text(hwnd, IDC_ABOUT_TAGLINE, &fl!("about-tagline"));
            controls::set_text(hwnd, IDC_ABOUT_DESCRIPTION, &fl!("about-description"));
            controls::set_text(hwnd, IDC_ABOUT_COPYRIGHT, &fl!("about-copyright"));
            controls::set_text(
                hwnd,
                IDC_ABOUT_LICENSE,
                &fl!("about-license", license = "<a>MPL-2.0</a>"),
            );
            let repository = env!("CARGO_PKG_REPOSITORY");
            let address = repository.strip_prefix("https://").unwrap_or(repository);
            controls::set_text(
                hwnd,
                IDC_ABOUT_LINK,
                &fl!("about-source", address = format!("<a>{address}</a>")),
            );
        }
    }
}

/// Shows the current state on `page`.
///
/// With `force` unset, lists are rebuilt only when their contents changed.
/// Callers wrap it in [`super::quietly`] so the control changes it makes are
/// not taken for user input.
pub(super) fn refresh(page: Page, hwnd: HWND, force: bool) {
    let state = state();
    let config = state.config();
    match page {
        Page::Overview => {
            controls::set_checked(hwnd, IDC_ENABLED, config.enabled);
            let status_text = if config.enabled {
                fl!("overview-status-on")
            } else {
                fl!("overview-status-off")
            };
            controls::set_text(hwnd, IDC_STATUS, &status_text);
            let status = state.status.lock();
            let focus = match &status.current {
                Some((process, support)) => format!("{process}: {}", support_label(*support)),
                None => fl!("overview-focus-none"),
            };
            controls::set_text(hwnd, IDC_FOCUS, &focus);
            controls::set_text(
                hwnd,
                IDC_COUNT,
                &fl!("overview-count", count = status.count),
            );
        }
        Page::Rules => {
            controls::set_checked(hwnd, IDC_ENGINE_RULES, config.engine == Engine::Rules);
            controls::set_checked(
                hwnd,
                IDC_ENGINE_MLM,
                config.engine == Engine::MaskedLanguageModel,
            );
            let list = state.rules.lock().clone();
            controls::set_text(hwnd, IDC_RULES_STATUS, &rules_status(&list));
            // The list view only asks for the rows it shows through
            // LVS_OWNERDATA, so even a huge rule file is listed instantly.
            if changed(
                page,
                (&list.loaded_at, list.rules.len(), list.missing),
                force,
            ) {
                ListView::new(hwnd, IDC_RULES).set_virtual_len(list.rules.len());
            }
        }
        Page::Model => refresh_model(hwnd, &config, force),
        Page::Usage => sync_usage_switch(hwnd, &config),
        Page::Apps => {
            let rows = app_rows(&state.status.lock().apps, &config);
            if changed(page, &rows, force) {
                let list = ListView::new(hwnd, IDC_APPS);
                // Rebuilding the list drops the selection, which the mode
                // box below it follows, so carry it over by program name.
                let selected = list.selected().map(|i| list.text(i, 0));
                list.clear();
                for (process, mode, status) in &rows {
                    list.push(&[process.clone(), mode_label(*mode), status.clone()]);
                }
                if let Some(index) =
                    selected.and_then(|name| rows.iter().position(|(p, ..)| *p == name))
                {
                    list.select(index);
                }
            }
            sync_app_mode(hwnd, &config);
        }
        Page::History => {
            let status = state.status.lock();
            let entries: Vec<[String; 4]> = status
                .history
                .iter()
                .map(|e| {
                    [
                        e.time.clone(),
                        e.record.process.clone(),
                        format!("{} → {}", e.record.from, e.record.to),
                        outcome_label(e.record.outcome),
                    ]
                })
                .collect();
            drop(status);
            if changed(page, (&entries, config.private_mode), force) {
                let list = ListView::new(hwnd, IDC_HISTORY);
                list.clear();
                for entry in &entries {
                    list.push(entry);
                }
            }
            let empty = if config.private_mode {
                fl!("history-private")
            } else if entries.is_empty() {
                fl!("history-empty")
            } else {
                String::new()
            };
            controls::set_text(hwnd, IDC_HISTORY_EMPTY, &empty);
        }
        Page::Settings => {
            if force {
                controls::set_checked(hwnd, IDC_AUTOSTART, platform::autostart_enabled());
                controls::set_checked(hwnd, IDC_PRIVATE, config.private_mode);
                controls::set_checked(hwnd, IDC_BLIND, config.blind_mode);
                controls::spin_setup(
                    hwnd,
                    IDC_IDLE_SPIN,
                    IDLE_RANGE.0,
                    IDLE_RANGE.1,
                    config.idle_ms as i32,
                );
                controls::spin_setup(
                    hwnd,
                    IDC_CARET_SPIN,
                    CARET_RANGE.0,
                    CARET_RANGE.1,
                    config.caret_window as i32,
                );
            }
        }
        Page::About => {}
    }
}

/// Handles notification `code` of control `id` on `page` by writing the
/// change to the shared state.
pub(super) fn command(page: Page, hwnd: HWND, id: i32, code: u32) {
    let state = state();
    match (page, id, code) {
        (Page::Overview, IDC_ENABLED, BN_CLICKED) => {
            let enabled = controls::checked(hwnd, IDC_ENABLED);
            state.update_config(|c| c.enabled = enabled);
            tray::sync(&state);
            super::quietly(|| refresh(page, hwnd, true));
        }
        (Page::Rules, IDC_ENGINE_RULES, BN_CLICKED) => {
            state.update_config(|c| c.engine = Engine::Rules);
        }
        (Page::Rules, IDC_ENGINE_MLM, BN_CLICKED) => {
            state.update_config(|c| c.engine = Engine::MaskedLanguageModel);
        }
        (Page::Rules, IDC_EDIT_RULES, BN_CLICKED) => {
            // Recreate the file if the user deleted it, so the editor never
            // opens on nothing. The watcher picks the new file up.
            if let Err(e) = rules::ensure_exists() {
                tracing::error!("creating the rule file failed: {e}");
            }
            if let Some(path) = rules::path() {
                platform::open_file(&path);
            }
        }
        (Page::Rules, IDC_OPEN_RULES_FOLDER, BN_CLICKED) => {
            if let Some(dir) = rules::path().as_deref().and_then(|p| p.parent()) {
                platform::open_folder(dir);
            }
        }
        (Page::Model, IDC_LM_MODEL, CBN_SELCHANGE) => {
            let Some(id) = controls::combo_selected(hwnd, IDC_LM_MODEL)
                .and_then(|i| MODEL_IDS.with(|ids| ids.borrow().get(i).cloned()))
            else {
                return;
            };
            state.update_config(|c| {
                if let Some(model) = lm::model_info(&id) {
                    c.lm.simplify = model.simplify;
                }
                c.lm.model = id;
            });
        }
        (Page::Model, IDC_LM_MODE, CBN_SELCHANGE) => {
            let Some(&mode) =
                controls::combo_selected(hwnd, IDC_LM_MODE).and_then(|i| LmMode::ALL.get(i))
            else {
                return;
            };
            state.update_config(|c| c.lm.mode = mode);
        }
        (Page::Model, IDC_LM_DOWNLOAD, BN_CLICKED) => {
            if matches!(
                state.engines.download_state(),
                Some(DownloadState::Running { .. })
            ) {
                state.engines.cancel_download();
                return;
            }
            let config = state.config();
            if let (Some(model), Some(dir)) = (
                lm::model_info(&config.lm.model),
                lm::model_dir(&config.lm, &config.lm.model),
            ) {
                state.engines.start_download(model, dir);
            }
        }
        (Page::Model, IDC_LM_BENCH, BN_CLICKED) => super::bench_window::show(),
        (Page::Model, IDC_LM_DELETE, BN_CLICKED) => {
            let config = state.config();
            let Some(dir) = lm::model_dir(&config.lm, &config.lm.model) else {
                return;
            };
            let question = fl!("lm-confirm-delete", model = config.lm.model.clone());
            if !platform::confirm(hwnd, &question) {
                return;
            }
            if let Err(e) = state.engines.delete_model(&dir) {
                platform::message_box(hwnd, &fl!("lm-delete-failed", error = e.to_string()));
            }
            super::quietly(|| refresh(page, hwnd, true));
        }
        (Page::Model, IDC_LM_CHANGE_DIR, BN_CLICKED) => {
            let current = lm::models_dir(&state.config().lm);
            let Some(dir) = platform::pick_folder(hwnd, current.as_deref()) else {
                return;
            };
            let dir = (Some(&dir) != lm::default_models_dir().as_ref()).then_some(dir);
            state.update_config(|c| c.lm.models_dir = dir);
            super::quietly(|| refresh(page, hwnd, true));
        }
        (Page::Model, IDC_OPEN_MODELS_FOLDER, BN_CLICKED) => {
            if let Some(dir) = lm::models_dir(&state.config().lm) {
                platform::open_folder(&dir);
            }
        }
        (Page::Model, IDC_LM_TRY, BN_CLICKED) => super::try_window::show(),
        (Page::Usage, IDC_USAGE_ENABLED, BN_CLICKED) => {
            let enabled = controls::checked(hwnd, IDC_USAGE_ENABLED);
            state.update_config(|c| c.usage_monitor = enabled);
            super::quietly(|| refresh(page, hwnd, true));
        }
        (Page::Apps, IDC_APP_MODE, CBN_SELCHANGE) => {
            let list = ListView::new(hwnd, IDC_APPS);
            let (Some(row), Some(index)) = (
                list.selected(),
                controls::combo_selected(hwnd, IDC_APP_MODE),
            ) else {
                return;
            };
            let process = list.text(row, 0);
            let mode = AppMode::ALL[index.min(AppMode::ALL.len() - 1)];
            state.update_config(|c| c.set_app_mode(&process, mode));
        }
        (Page::History, IDC_CLEAR, BN_CLICKED) => {
            state.status.lock().history.clear();
            super::quietly(|| refresh(page, hwnd, true));
        }
        (Page::Settings, IDC_LANGUAGE, CBN_SELCHANGE) => {
            let Some(index) = controls::combo_selected(hwnd, IDC_LANGUAGE) else {
                return;
            };
            let language = if index == 0 {
                Language::System
            } else {
                Language::CHOICES[index - 1]
            };
            state.update_config(|c| c.language = language);
            crate::i18n::apply(language);
            // Relabeling rebuilds this combo box, which must not happen while
            // it is still delivering its own notification.
            crate::app::post(crate::app::WM_APP_RELABEL);
        }
        (Page::Settings, IDC_AUTOSTART, BN_CLICKED) => {
            if let Err(e) = platform::set_autostart(controls::checked(hwnd, IDC_AUTOSTART)) {
                tracing::error!("changing autostart failed: {e}");
            }
            tray::sync(&state);
        }
        (Page::Settings, IDC_PRIVATE, BN_CLICKED) => {
            let private = controls::checked(hwnd, IDC_PRIVATE);
            state.update_config(|c| c.private_mode = private);
            tray::sync(&state);
        }
        (Page::Settings, IDC_DELETE_LOGS, BN_CLICKED) => {
            let message = match logging::delete_all() {
                Ok(count) => fl!("settings-logs-deleted", count = count),
                Err(e) => fl!("settings-logs-delete-failed", error = e.to_string()),
            };
            platform::message_box(hwnd, &message);
        }
        (Page::Settings, IDC_BLIND, BN_CLICKED) => {
            let blind = controls::checked(hwnd, IDC_BLIND);
            state.update_config(|c| c.blind_mode = blind);
        }
        (Page::Settings, IDC_IDLE, EN_CHANGE) => {
            if let Some(value) = controls::spin_value(hwnd, IDC_IDLE_SPIN) {
                state.update_config(|c| c.idle_ms = value as u64);
            }
        }
        (Page::Settings, IDC_CARET, EN_CHANGE) => {
            if let Some(value) = controls::spin_value(hwnd, IDC_CARET_SPIN) {
                state.update_config(|c| c.caret_window = value as usize);
            }
        }
        (Page::Settings, IDC_OPEN_LOGS, BN_CLICKED) => {
            if let Some(dir) = logging::dir() {
                platform::open_folder(&dir);
            }
        }
        _ => {}
    }
}

/// Handles a `WM_NOTIFY` message described by `header` on `page`.
///
/// `lparam` is the pointer the message carried. It is read only for codes
/// whose notification structure is larger than [`NMHDR`].
pub(super) fn notify(page: Page, hwnd: HWND, header: &NMHDR, lparam: LPARAM) {
    let id = header.idFrom as i32;
    match (page, id, header.code) {
        (Page::Apps, IDC_APPS, LVN_ITEMCHANGED) => {
            // SAFETY: for LVN_ITEMCHANGED, `lparam` points to an NMLISTVIEW
            // that stays valid for the duration of the message.
            let change = unsafe { &*(lparam.0 as *const NMLISTVIEW) };
            let selection_changed = change.uChanged.0 & LVIF_STATE != 0
                && (change.uNewState ^ change.uOldState) & LVIS_SELECTED != 0;
            if selection_changed {
                let config = state().config();
                super::quietly(|| sync_app_mode(hwnd, &config));
            }
        }
        (Page::About, IDC_ABOUT_LINK, NM_CLICK | NM_RETURN) => {
            platform::open_url(env!("CARGO_PKG_REPOSITORY"));
        }
        (Page::About, IDC_ABOUT_LICENSE, NM_CLICK | NM_RETURN) => {
            platform::open_url(LICENSE_URL);
        }
        _ => {}
    }
}

/// Supplies the text of one cell of the virtual rule list.
///
/// Must run even while the page is updating, since the list view asks for
/// text whenever it repaints.
pub(super) fn display_info(page: Page, lparam: LPARAM) {
    if page != Page::Rules {
        return;
    }
    // SAFETY: for LVN_GETDISPINFOW, `lparam` points to an NMLVDISPINFOW that
    // is valid and exclusively ours for the duration of the message. Its text
    // buffer, when requested, holds cchTextMax UTF-16 units.
    let info = unsafe { &mut *(lparam.0 as *mut NMLVDISPINFOW) };
    if info.item.mask.0 & LVIF_TEXT == 0 || info.item.pszText.is_null() || info.item.cchTextMax <= 0
    {
        return;
    }
    let rules = state().rules.lock().rules.clone();
    let text = usize::try_from(info.item.iItem)
        .ok()
        .and_then(|i| rules.get(i))
        .map(|rule| {
            if info.item.iSubItem == 0 {
                rule.from.as_str()
            } else {
                rule.to.as_str()
            }
        })
        .unwrap_or_default();
    let capacity = info.item.cchTextMax as usize - 1;
    let units: Vec<u16> = text.encode_utf16().take(capacity).collect();
    // SAFETY: `capacity` is one less than the buffer size, and `units` holds
    // at most that many, so the copy and the terminator stay inside the
    // buffer. The buffer cannot overlap `units`, which is a local copy.
    unsafe {
        std::ptr::copy_nonoverlapping(units.as_ptr(), info.item.pszText.0, units.len());
        *info.item.pszText.0.add(units.len()) = 0;
    }
}

/// Describes the loaded rule file: how many rules, when, and the first line
/// that could not be read.
fn rules_status(list: &RuleList) -> String {
    if list.missing {
        return fl!("rules-status-missing");
    }
    let loaded = fl!(
        "rules-status",
        count = list.rules.len(),
        time = list.loaded_at.clone()
    );
    let Some(first) = list.errors.first() else {
        return loaded;
    };
    let reason = match first.kind {
        ParseErrorKind::MissingSeparator => fl!("rules-error-separator"),
        ParseErrorKind::EmptySide => fl!("rules-error-empty"),
    };
    let problem = fl!(
        "rules-status-error",
        line = first.line,
        reason = reason,
        count = list.errors.len()
    );
    format!("{loaded}\n{problem}")
}

/// Releases what `page` holds outside its window: the fingerprint of its
/// list, the resource sampler of the Usage page, and the GDI objects of the
/// About page.
pub(super) fn destroy(page: Page, _hwnd: HWND) {
    SHOWN.with(|s| s.borrow_mut().remove(&page));
    if page == Page::Usage {
        state().monitor.stop();
    }
    if page == Page::About
        && let Some(font) = TITLE_FONT.with(|f| f.borrow_mut().take())
    {
        // SAFETY: the font was created by set_title_font and taken out of
        // TITLE_FONT, so it is deleted once, and the control that used it is
        // being destroyed.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
    if page == Page::About
        && let Some(logo) = ABOUT_LOGO.with(|l| l.borrow_mut().take())
    {
        // SAFETY: the icon was loaded by set_logo and taken out of
        // ABOUT_LOGO, so it is destroyed once, and its control is going away.
        unsafe {
            let _ = DestroyIcon(logo);
        }
    }
}

/// Shows the program icon on the About page, 24 dialog units high so it
/// scales with the dialog font and the monitor's DPI.
fn set_logo(hwnd: HWND) {
    let mut rect = RECT {
        bottom: 24,
        ..Default::default()
    };
    // SAFETY: `hwnd` is the page dialog being initialized, and `rect` is a
    // valid pointer that the call converts in place.
    if unsafe { MapDialogRect(hwnd, &mut rect) }.is_err() {
        return;
    }
    match icon::load(icon::IDI_APP, rect.bottom) {
        Ok(logo) => {
            let control = controls::item(hwnd, IDC_ABOUT_LOGO);
            controls::send(control, STM_SETICON, logo.0 as usize, 0);
            ABOUT_LOGO.with(|l| *l.borrow_mut() = Some(logo));
        }
        Err(e) => tracing::warn!("loading the About icon failed: {e}"),
    }
}

/// Shows the program name on the About page in a larger, bold version of
/// the dialog font.
fn set_title_font(hwnd: HWND) {
    let label = controls::item(hwnd, IDC_ABOUT_NAME);
    let base = HFONT(controls::send(hwnd, WM_GETFONT, 0, 0) as *mut _);
    let mut font = LOGFONTW::default();
    // SAFETY: the size passed is that of `font`, which is a valid writable
    // LOGFONTW. A null or foreign font handle only makes the call return 0.
    let read = unsafe {
        GetObjectW(
            HGDIOBJ(base.0),
            size_of::<LOGFONTW>() as i32,
            Some(&mut font as *mut _ as *mut _),
        )
    };
    if read == 0 {
        return;
    }
    font.lfHeight = font.lfHeight * 8 / 5;
    font.lfWeight = FW_BOLD.0 as i32;
    // SAFETY: `font` is a fully initialized LOGFONTW. The font created is
    // stored in TITLE_FONT, from which `destroy` deletes it.
    let title = unsafe { CreateFontIndirectW(&font) };
    controls::send(label, WM_SETFONT, title.0 as usize, 1);
    TITLE_FONT.with(|f| *f.borrow_mut() = Some(title));
}

/// Returns the position of `language` in the language box, whose first entry
/// is the system language and is followed by [`Language::CHOICES`].
fn language_index(language: Language) -> usize {
    Language::CHOICES
        .iter()
        .position(|&l| l == language)
        .map_or(0, |i| i + 1)
}

/// Returns a row of name, mode and status for every program seen since start
/// and every program that has a mode of its own, sorted by name.
fn app_rows(
    seen: &BTreeMap<String, (Support, AppMode)>,
    config: &Config,
) -> Vec<(String, AppMode, String)> {
    let mut rows: Vec<(String, AppMode, String)> = seen
        .iter()
        .map(|(process, &(support, seen_mode))| {
            let mode = config.app_mode(process);
            // How the program was handled says nothing about a mode chosen
            // since, which only takes effect when it is used again.
            let status = if mode == AppMode::Off {
                support_label(Support::Excluded(ExcludeReason::DisabledByUser))
            } else if mode != seen_mode {
                fl!("apps-pending")
            } else {
                support_label(support)
            };
            (process.clone(), mode, status)
        })
        .collect();
    for (process, &mode) in &config.apps {
        if !seen.keys().any(|p| p.eq_ignore_ascii_case(process)) {
            rows.push((process.clone(), mode, fl!("apps-not-seen")));
        }
    }
    rows.sort_by_key(|(p, ..)| p.to_lowercase());
    rows
}

/// Points the mode box and its hint at the selected program.
fn sync_app_mode(hwnd: HWND, config: &Config) {
    let list = ListView::new(hwnd, IDC_APPS);
    let mode = list.selected().map(|i| config.app_mode(&list.text(i, 0)));
    controls::enable(hwnd, IDC_APP_MODE, mode.is_some());
    controls::combo_select(
        hwnd,
        IDC_APP_MODE,
        mode.and_then(|m| AppMode::ALL.iter().position(|&a| a == m)),
    );
    let hint = match mode {
        Some(mode) => mode_hint(mode),
        None if list.len() == 0 => fl!("apps-empty"),
        None => fl!("apps-select-hint"),
    };
    controls::set_text(hwnd, IDC_APP_MODE_HINT, &hint);
}

/// Fills the model box with the known models, marking those not downloaded
/// yet, followed by the chosen model when it is not one of them.
///
/// The known models are those in [`typomend_scorer::MODELS`], which download
/// as safetensors. Records the model ids in the order of the entries for
/// `command` to map a selection back.
fn fill_models(hwnd: HWND) {
    let config = state().config();
    let mut ids: Vec<String> = typomend_scorer::MODELS
        .iter()
        .map(|m| m.id.to_owned())
        .collect();
    if !ids.contains(&config.lm.model) {
        ids.push(config.lm.model.clone());
    }
    let items: Vec<String> = ids
        .iter()
        .map(|id| {
            let name = match lm::model_info(id) {
                Some(m) if lm::is_installed(&config.lm, id) => {
                    format!("{} ({})", m.id, m.license)
                }
                Some(m) => format!(
                    "{} ({}, {} MB) {}",
                    m.id,
                    m.license,
                    m.size_mb,
                    fl!("lm-not-installed")
                ),
                None => id.clone(),
            };
            match measured(id, config.lm.mode) {
                Some(m) => format!(
                    "{name}    {}",
                    fl!(
                        "lm-model-summary",
                        tw = m[0].fixed.to_string(),
                        cn = m[1].fixed.to_string(),
                        mixed = m[2].fixed.to_string()
                    )
                ),
                None => name,
            }
        })
        .collect();
    let selected = ids
        .iter()
        .position(|id| *id == config.lm.model)
        .unwrap_or(0);
    controls::combo_fill(hwnd, IDC_LM_MODEL, &items, selected);
    MODEL_IDS.with(|m| *m.borrow_mut() = ids);
}

/// Shows the state of the Masked language model page: selection, download and
/// delete buttons, folder and status.
///
/// Deleting is disabled while a download or a benchmark may be using the
/// files.
fn refresh_model(hwnd: HWND, config: &Config, force: bool) {
    let state = state();
    let download = state.engines.download_state();
    let downloading = matches!(download, Some(DownloadState::Running { .. }));
    let model = lm::model_info(&config.lm.model);
    let installed = lm::is_installed(&config.lm, &config.lm.model);

    // Relabel the model box when a download finished or the folder changed.
    let flags: Vec<bool> = typomend_scorer::MODELS
        .iter()
        .map(|m| lm::is_installed(&config.lm, m.id))
        .collect();
    if changed(
        Page::Model,
        (&flags, &config.lm.models_dir, config.lm.mode as u8),
        force,
    ) {
        fill_models(hwnd);
    }
    let ids = MODEL_IDS.with(|ids| ids.borrow().clone());
    controls::combo_select(
        hwnd,
        IDC_LM_MODEL,
        ids.iter().position(|id| *id == config.lm.model),
    );
    controls::combo_select(
        hwnd,
        IDC_LM_MODE,
        LmMode::ALL.iter().position(|&m| m == config.lm.mode),
    );

    let download_label = if downloading {
        fl!("lm-cancel")
    } else {
        fl!("lm-download")
    };
    controls::set_text(hwnd, IDC_LM_DOWNLOAD, &download_label);
    controls::enable(
        hwnd,
        IDC_LM_DOWNLOAD,
        downloading || (model.is_some() && !installed),
    );
    let bench = state.engines.bench_state();
    let benchmarking = matches!(bench, Some(BenchState::Running { .. }));
    controls::enable(
        hwnd,
        IDC_LM_DELETE,
        model.is_some() && installed && !downloading && !benchmarking,
    );
    controls::enable(hwnd, IDC_LM_CHANGE_DIR, !downloading);

    let dir = lm::models_dir(&config.lm)
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    controls::set_text(hwnd, IDC_LM_DIR, &fl!("lm-dir", path = dir));
    controls::set_text(hwnd, IDC_LM_EXPECTED, &expected(config, bench.as_ref()));
    let status = bench_status(config, bench.as_ref())
        .unwrap_or_else(|| lm_status(config, download, installed));
    controls::set_text(hwnd, IDC_LM_STATUS, &status);
}

/// Returns the measured results of `id` in `mode` on the bundled test sets,
/// in the order of [`typomend_scorer::bench::TEST_SETS`].
pub(super) fn measured(id: &str, mode: LmMode) -> Option<&'static [Measured; 3]> {
    lm::model_info(id).map(|m| m.measured(mode.into()))
}

/// Describes how well the chosen model and mode correct each language: as
/// measured on this computer when the user ran the benchmark with these
/// settings, otherwise as measured when the model was added.
fn expected(config: &Config, bench: Option<&BenchState>) -> String {
    let mode = lm_mode_name(config.lm.mode);
    let line = |set: &str, fixed: f32, fp: f32, ms: u128| {
        fl!(
            "lm-bench-result-line",
            set = set_label(set),
            fixed = format!("{fixed:.0}"),
            fp = format!("{fp:.0}"),
            ms = ms.to_string()
        )
    };
    if let Some(BenchState::Done {
        config: ran,
        results,
    }) = bench
        && *ran == config.lm
    {
        let mut text = fl!("lm-measured-here", mode = mode);
        for (set, outcome) in results {
            text.push('\n');
            text.push_str(&line(
                set,
                outcome.fixed_rate() * 100.0,
                outcome.false_positive_rate() * 100.0,
                outcome.mean_call().as_millis(),
            ));
        }
        return text;
    }
    let Some(results) = measured(&config.lm.model, config.lm.mode) else {
        return fl!("lm-expected-none");
    };
    let mut text = fl!("lm-expected", mode = mode);
    for (set, m) in TEST_SETS.iter().zip(results) {
        text.push('\n');
        text.push_str(&line(
            set.id,
            f32::from(m.fixed),
            f32::from(m.false_positives),
            u128::from(m.ms),
        ));
    }
    text
}

/// Describes a running benchmark, or why the last one failed.
fn bench_status(config: &Config, bench: Option<&BenchState>) -> Option<String> {
    match bench? {
        BenchState::Running {
            model,
            set,
            done,
            total,
        } => Some(fl!(
            "lm-status-benchmarking",
            model = model.clone(),
            set = set_label(set),
            done = done.to_string(),
            total = total.to_string()
        )),
        BenchState::Failed { model, error } if *model == config.lm.model => {
            Some(fl!("lm-bench-failed", error = error.clone()))
        }
        _ => None,
    }
}

/// Returns the caption of test set `id`, or `id` itself when it has none.
pub(super) fn set_label(id: &str) -> String {
    match id {
        "zh-TW" => fl!("lm-set-zh-tw"),
        "zh-CN" => fl!("lm-set-zh-cn"),
        "zh-TW-mixed" => fl!("lm-set-zh-tw-mixed"),
        other => other.to_owned(),
    }
}

/// Returns the short name of `mode`, for use inside a sentence.
pub(super) fn lm_mode_name(mode: LmMode) -> String {
    match mode {
        LmMode::Accurate => fl!("lm-mode-name-accurate"),
        LmMode::Balanced => fl!("lm-mode-name-balanced"),
        LmMode::Fast => fl!("lm-mode-name-fast"),
    }
}

/// Returns the entry of `mode` in the mode box.
fn lm_mode_label(mode: LmMode) -> String {
    match mode {
        LmMode::Accurate => fl!("lm-mode-accurate"),
        LmMode::Balanced => fl!("lm-mode-balanced"),
        LmMode::Fast => fl!("lm-mode-fast"),
    }
}

/// Returns the status line of the Masked language model page.
///
/// A download in progress, or one that failed for the chosen model, takes
/// precedence over the state of the installed model.
fn lm_status(config: &Config, download: Option<DownloadState>, installed: bool) -> String {
    match download {
        Some(DownloadState::Running { id, progress }) => {
            let Some(p) = progress else {
                return fl!("lm-status-connecting", model = id);
            };
            let mb = |bytes: u64| (bytes >> 20).to_string();
            let total = p.total.unwrap_or(0);
            let percent = (p.done * 100).checked_div(total).unwrap_or(0);
            return fl!(
                "lm-status-downloading",
                model = id,
                done = mb(p.done),
                total = mb(total),
                percent = percent.to_string(),
                speed = format!("{:.1}", p.speed / 1_048_576.0),
                threads = p.threads.to_string()
            );
        }
        Some(DownloadState::Failed { id, error }) if id == config.lm.model => {
            return fl!("lm-status-download-failed", error = error);
        }
        _ => {}
    }
    if !installed {
        return lm::model_info(&config.lm.model)
            .map(|m| fl!("lm-status-missing", size = m.size_mb.to_string()))
            .unwrap_or_default();
    }
    match state().engines.status() {
        LmStatus::Off | LmStatus::Missing => fl!("lm-status-off"),
        LmStatus::Loading => fl!("lm-status-loading"),
        LmStatus::Ready => fl!("lm-status-ready"),
        LmStatus::Failed(error) => fl!("lm-status-failed", error = error),
    }
}

fn mode_label(mode: AppMode) -> String {
    match mode {
        AppMode::Auto => fl!("mode-auto"),
        AppMode::Standard => fl!("mode-standard"),
        AppMode::Blind => fl!("mode-blind"),
        AppMode::Terminal => fl!("mode-terminal"),
        AppMode::Off => fl!("mode-off"),
    }
}

fn mode_hint(mode: AppMode) -> String {
    match mode {
        AppMode::Auto => fl!("mode-auto-hint"),
        AppMode::Standard => fl!("mode-standard-hint"),
        AppMode::Blind => fl!("mode-blind-hint"),
        AppMode::Terminal => fl!("mode-terminal-hint"),
        AppMode::Off => fl!("mode-off-hint"),
    }
}

fn support_label(support: Support) -> String {
    match support {
        Support::Readable => fl!("support-readable"),
        Support::Blind => fl!("support-blind"),
        Support::Terminal => fl!("support-terminal"),
        Support::Unsupported => fl!("support-unsupported"),
        Support::Excluded(ExcludeReason::Password) => fl!("support-excluded-password"),
        Support::Excluded(ExcludeReason::DisabledByUser) => fl!("support-excluded-disabled"),
        Support::Excluded(ExcludeReason::Terminal) => fl!("support-excluded-terminal"),
        Support::Excluded(ExcludeReason::Elevated) => fl!("support-excluded-elevated"),
        Support::Excluded(ExcludeReason::BlindModeOff) => fl!("support-excluded-blind-off"),
    }
}

fn outcome_label(outcome: Outcome) -> String {
    match outcome {
        Outcome::Verified => fl!("outcome-verified"),
        Outcome::Unverified => fl!("outcome-unverified"),
        Outcome::Failed => fl!("outcome-failed"),
        Outcome::Skipped => fl!("outcome-skipped"),
        Outcome::TooSlow => fl!("outcome-too-slow"),
    }
}

/// Charts on the Usage page, top to bottom.
const CHARTS: [i32; 3] = [IDC_USAGE_CPU, IDC_USAGE_MEMORY, IDC_USAGE_GPU];

/// Shows the switch and starts or stops sampling to match it.
///
/// The charts are visible only while sampling.
fn sync_usage_switch(hwnd: HWND, config: &Config) {
    let monitor = &state().monitor;
    if config.usage_monitor {
        monitor.start();
    } else {
        monitor.stop();
    }
    controls::set_checked(hwnd, IDC_USAGE_ENABLED, config.usage_monitor);
    for id in CHARTS {
        controls::show(hwnd, id, config.usage_monitor);
    }
    refresh_usage(hwnd);
}

/// Updates the Usage page from the latest samples and repaints its charts.
pub(super) fn refresh_usage(hwnd: HWND) {
    let state = state();
    let mut text = model_usage(&state.config());
    if let Some(last) = state.monitor.samples().last() {
        text.push('\n');
        text.push_str(&gpu_memory(last));
    }
    controls::set_text(hwnd, IDC_USAGE_MODEL, &text);
    for id in CHARTS {
        // SAFETY: no rectangle is passed, so the whole control is
        // invalidated, and a null handle only makes the call fail. The chart
        // then repaints through WM_DRAWITEM.
        unsafe {
            let _ = InvalidateRect(Some(controls::item(hwnd, id)), None, false);
        }
    }
}

/// Describes where the masked language model runs and the memory it took.
fn model_usage(config: &Config) -> String {
    let state = state();
    let Some(lm_config) = config.wanted_lm() else {
        return fl!("usage-model-none");
    };
    match state.engines.status() {
        LmStatus::Ready => {}
        LmStatus::Loading => return fl!("usage-model-loading"),
        _ => return fl!("usage-model-none"),
    }
    match state.engines.model_memory() {
        Some(bytes) => fl!(
            "usage-model-memory",
            model = lm_config.model,
            size = (bytes >> 20).to_string()
        ),
        None => fl!("usage-model-memory-unknown", model = lm_config.model),
    }
}

/// Describes the GPU memory Typomend used in `usage`, or that none is
/// available.
fn gpu_memory(usage: &Usage) -> String {
    match usage.gpu_memory {
        Some(bytes) => fl!("usage-gpu-memory", typomend = (bytes >> 20).to_string()),
        None => fl!("usage-gpu-none"),
    }
}

/// What one chart on the Usage page plots.
struct ChartSpec {
    /// Extracts the plotted value from a sample, `None` when it is missing.
    pick: fn(&Usage) -> Option<f32>,
    /// The least the scale goes up to, so idle noise stays flat.
    floor: f32,
    /// Builds the chart title from the top of the scale.
    title: fn(String) -> String,
    /// Formats a value for the legend.
    format: fn(f32) -> String,
}

/// Paints the owner-drawn control `item`.
///
/// Returns whether the control belongs to this page, which tells the dialog
/// procedure whether the message was handled. Each chart plots Typomend's own
/// process as a single line.
pub(super) fn draw_item(page: Page, hwnd: HWND, item: &DRAWITEMSTRUCT) -> bool {
    if page != Page::Usage {
        return false;
    }
    let spec = match item.CtlID as i32 {
        IDC_USAGE_CPU => ChartSpec {
            pick: |u| Some(u.cpu),
            floor: 5.0,
            title: |max| fl!("usage-cpu-title", max = max),
            format: |v| format!("{v:.1}%"),
        },
        IDC_USAGE_MEMORY => ChartSpec {
            pick: |u| Some(u.memory as f32 / 1_048_576.0),
            floor: 100.0,
            title: |max| fl!("usage-memory-title", max = max),
            format: |v| format!("{v:.0} MB"),
        },
        IDC_USAGE_GPU => ChartSpec {
            pick: |u| u.gpu,
            floor: 5.0,
            title: |max| fl!("usage-gpu-title", max = max),
            format: |v| format!("{v:.0}%"),
        },
        _ => return false,
    };
    let ChartSpec {
        pick,
        floor,
        title,
        format,
    } = spec;
    let samples = state().monitor.samples();
    let mut series = Vec::new();
    let own: Vec<f32> = samples.iter().filter_map(pick).collect();
    if let Some(&last) = own.last() {
        series.push(Series {
            legend: format!("Typomend {}", format(last)),
            color: chart::BLUE,
            values: own,
        });
    }
    let peak = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .fold(0.0, f32::max);
    let max = chart::nice_max(peak, floor);
    // Without GPU readings the title says so instead of showing a scale.
    let title = if item.CtlID as i32 == IDC_USAGE_GPU && !samples.is_empty() && series.is_empty() {
        fl!("usage-gpu-none")
    } else {
        title(max.to_string())
    };
    let font = HFONT(controls::send(hwnd, WM_GETFONT, 0, 0) as *mut _);
    chart::draw(
        item,
        font,
        &Chart {
            title,
            max,
            capacity: monitor::HISTORY,
            series: &series,
        },
    );
    true
}
