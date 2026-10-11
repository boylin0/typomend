// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Typomend fixes typos after the input method commits them, in any app.
//!
//! The program lives in the notification area. Its settings window is a
//! native Win32 property sheet; the work happens on the observer thread from
//! `typomend-observer`.

// Release builds are GUI programs without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod config;
#[cfg(windows)]
mod dialog;
#[cfg(windows)]
mod i18n;
#[cfg(windows)]
mod icon;
#[cfg(windows)]
mod lm;
#[cfg(windows)]
mod logging;
#[cfg(windows)]
mod monitor;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod rules;
#[cfg(windows)]
mod state;
#[cfg(windows)]
mod tray;

#[cfg(windows)]
use typomend_observer::Observer;
#[cfg(windows)]
use windows::Win32::Foundation::HWND;

#[cfg(windows)]
use crate::config::Config;
#[cfg(windows)]
use crate::state::AppState;

/// Starts Typomend, or tells the running instance to show its window.
///
/// With `--background` the settings window stays closed, which is how the Run
/// key starts the program at sign-in.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    // Started by the Run key at sign-in: stay in the notification area.
    let background = std::env::args().any(|a| a == "--background");

    let Some(_instance) = platform::claim_single_instance() else {
        // Starting again from the Run key at sign-in is not something the
        // user did, so it stays silent.
        if !background {
            i18n::apply(Config::load().language);
            platform::message_box(HWND::default(), &fl!("already-running"));
            platform::show_running_instance();
        }
        return Ok(());
    };
    let config = Config::load();
    logging::init(config.private_mode, config.log.as_deref());
    i18n::apply(config.language);
    if let Err(e) = rules::ensure_exists() {
        tracing::error!("creating the rule file failed: {e}");
    }
    let state = AppState::new(config);
    state.reload_rules();
    {
        let state = state.clone();
        rules::watch(move || state.reload_rules());
    }

    app::create_main_window()?;
    dialog::init(state.clone());
    tray::create(state.clone())?;
    icon::watch_theme(|| app::post(app::WM_APP_THEME));
    let _observer = {
        let state = state.clone();
        Observer::spawn(state.shared.clone(), move |event| state.on_event(event))
    };

    if !background {
        dialog::show();
    }
    app::run_message_loop();
    Ok(())
}

/// Typomend depends on Windows APIs, so other platforms only build a stub.
#[cfg(not(windows))]
fn main() {
    eprintln!("Typomend runs only on Windows.");
    std::process::exit(1);
}
