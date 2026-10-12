// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runs the observer with the built-in rules and prints every event.
//!
//! Each argument sets the mode of one program, by executable name without
//! extension. The modes are `auto`, `standard`, `blind`, `terminal` and `off`.
//!
//! ```text
//! cargo run --example console -- [<program>=<mode>]...
//! cargo run --example console -- windowsterminal=terminal
//! ```
//!
//! Set `RUST_LOG=typomend_observer=trace` to also see composition updates.

#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
use tracing_subscriber::filter::{LevelFilter, Targets};
#[cfg(windows)]
use tracing_subscriber::prelude::*;

#[cfg(windows)]
use typomend_engine::RuleCorrector;
#[cfg(windows)]
use typomend_observer::{AppMode, Event, Observer, Settings, Shared};

#[cfg(windows)]
fn main() {
    let filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| s.parse::<Targets>().ok())
        .unwrap_or_else(|| Targets::new().with_default(LevelFilter::WARN));
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let corrector = Arc::new(RuleCorrector::new(RuleCorrector::builtin_rules()));
    let mut settings = Settings::default();
    for arg in std::env::args().skip(1) {
        let mode = arg.split_once('=').and_then(|(process, mode)| {
            let mode = match mode {
                "auto" => AppMode::Auto,
                "standard" => AppMode::Standard,
                "blind" => AppMode::Blind,
                "terminal" => AppMode::Terminal,
                "off" => AppMode::Off,
                _ => return None,
            };
            Some((process.to_ascii_lowercase(), mode))
        });
        let Some((process, mode)) = mode else {
            eprintln!("expected <program>=<auto|standard|blind|terminal|off>, got {arg}");
            std::process::exit(2);
        };
        settings.app_modes.insert(process, mode);
    }
    let shared = Shared::new(settings, corrector);
    let _observer = Observer::spawn(shared, |event| match event {
        Event::Focus { process, support } => println!("Focused: {process} {support:?}"),
        Event::Correction(r) => println!(
            "Correction: {} {} -> {} {:?}",
            r.process, r.from, r.to, r.outcome
        ),
    });
    println!("Typomend observer running. Press Ctrl+C to exit.");
    loop {
        std::thread::park();
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This example runs only on Windows.");
}
