// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resource identifiers of the dialog templates and their controls.
//!
//! These mirror `resources/resource.h`, which the resource compiler reads.
//! Keep the two files in sync.
//!
//! Control ids are grouped by page, and each group starts one above a
//! multiple of 100 so that an id identifies its page at a glance. Ids are unique across the
//! whole program because a control id is also what the dialog procedures
//! match on.

// Dialog templates, one per page or tool window.
pub const IDD_OVERVIEW: u16 = 101;
pub const IDD_RULES: u16 = 102;
pub const IDD_APPS: u16 = 103;
pub const IDD_HISTORY: u16 = 104;
pub const IDD_SETTINGS: u16 = 105;
pub const IDD_ABOUT: u16 = 106;
pub const IDD_MODEL: u16 = 107;
pub const IDD_USAGE: u16 = 108;
pub const IDD_TRY: u16 = 109;
pub const IDD_BENCH: u16 = 110;

// Overview page.
pub const IDC_ENABLED: i32 = 1001;
pub const IDC_STATUS: i32 = 1002;
pub const IDC_FOCUS_LABEL: i32 = 1003;
pub const IDC_FOCUS: i32 = 1004;
pub const IDC_COUNT: i32 = 1005;

// Rules page.
pub const IDC_ENGINE_LABEL: i32 = 1101;
pub const IDC_ENGINE_RULES: i32 = 1102;
pub const IDC_ENGINE_MLM: i32 = 1103;
pub const IDC_RULES_FILE: i32 = 1104;
pub const IDC_RULES_STATUS: i32 = 1105;
pub const IDC_RULES: i32 = 1106;
pub const IDC_EDIT_RULES: i32 = 1107;
pub const IDC_OPEN_RULES_FOLDER: i32 = 1108;

// Apps page.
pub const IDC_APPS_INTRO: i32 = 1201;
pub const IDC_APPS: i32 = 1202;
pub const IDC_APP_MODE_HINT: i32 = 1203;
pub const IDC_APP_MODE_LABEL: i32 = 1204;
pub const IDC_APP_MODE: i32 = 1205;

// History page.
pub const IDC_HISTORY_INTRO: i32 = 1301;
pub const IDC_HISTORY: i32 = 1302;
pub const IDC_HISTORY_EMPTY: i32 = 1303;
pub const IDC_CLEAR: i32 = 1304;

// Settings page.
pub const IDC_LANGUAGE_LABEL: i32 = 1401;
pub const IDC_LANGUAGE: i32 = 1402;
pub const IDC_AUTOSTART: i32 = 1403;
pub const IDC_PRIVATE: i32 = 1404;
pub const IDC_IDLE_LABEL: i32 = 1405;
pub const IDC_IDLE: i32 = 1406;
pub const IDC_IDLE_SPIN: i32 = 1407;
pub const IDC_IDLE_HINT: i32 = 1408;
pub const IDC_CARET_LABEL: i32 = 1409;
pub const IDC_CARET: i32 = 1410;
pub const IDC_CARET_SPIN: i32 = 1411;
pub const IDC_BLIND: i32 = 1412;
pub const IDC_BLIND_HINT: i32 = 1413;
pub const IDC_OPEN_LOGS: i32 = 1414;
pub const IDC_PRIVATE_HINT: i32 = 1415;
pub const IDC_DELETE_LOGS: i32 = 1416;

// About page.
pub const IDC_ABOUT_NAME: i32 = 1501;
pub const IDC_ABOUT_VERSION: i32 = 1502;
pub const IDC_ABOUT_DESCRIPTION: i32 = 1503;
pub const IDC_ABOUT_LICENSE: i32 = 1504;
pub const IDC_ABOUT_LINK: i32 = 1505;
pub const IDC_ABOUT_LOGO: i32 = 1506;
pub const IDC_ABOUT_TAGLINE: i32 = 1507;

// Masked language model page.
pub const IDC_LM_INTRO: i32 = 1601;
pub const IDC_LM_MODEL_LABEL: i32 = 1602;
pub const IDC_LM_MODEL: i32 = 1603;
pub const IDC_LM_MODE_LABEL: i32 = 1604;
pub const IDC_LM_MODE: i32 = 1605;
pub const IDC_LM_STATUS: i32 = 1606;
pub const IDC_LM_DOWNLOAD: i32 = 1607;
pub const IDC_LM_DELETE: i32 = 1608;
pub const IDC_LM_DIR: i32 = 1609;
pub const IDC_LM_CHANGE_DIR: i32 = 1610;
pub const IDC_OPEN_MODELS_FOLDER: i32 = 1611;
pub const IDC_LM_NOTE: i32 = 1612;
pub const IDC_LM_BENCH: i32 = 1613;
pub const IDC_LM_EXPECTED: i32 = 1614;
pub const IDC_LM_TRY: i32 = 1615;

// Usage page.
pub const IDC_USAGE_ENABLED: i32 = 1701;
pub const IDC_USAGE_HINT: i32 = 1702;
pub const IDC_USAGE_MODEL: i32 = 1703;
pub const IDC_USAGE_CPU: i32 = 1704;
pub const IDC_USAGE_MEMORY: i32 = 1705;
pub const IDC_USAGE_GPU: i32 = 1706;

// Try window, a tool window.
pub const IDC_TRY_INTRO: i32 = 1801;
pub const IDC_TRY_TEXT: i32 = 1802;
pub const IDC_TRY_RUN: i32 = 1803;
pub const IDC_TRY_RESULT: i32 = 1804;
pub const IDC_TRY_LIST: i32 = 1805;

// Accuracy benchmark window, a tool window.
pub const IDC_BENCH_INTRO: i32 = 1901;
pub const IDC_BENCH_MODEL: i32 = 1902;
pub const IDC_BENCH_RUN: i32 = 1903;
pub const IDC_BENCH_PROGRESS: i32 = 1904;
pub const IDC_BENCH_STATUS: i32 = 1905;
pub const IDC_BENCH_LIST: i32 = 1906;
