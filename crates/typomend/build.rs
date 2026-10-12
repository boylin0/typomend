// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiles `resources/typomend.rc` into the executable.
//!
//! The crate version is passed to the resource compiler as macros, so the
//! version information shown by File Explorer always matches `Cargo.toml`.
//! The manifest is required, because the settings window needs the version 6
//! common controls and per-monitor DPI awareness that it declares.

fn main() {
    // Build scripts run on the host, so the target decides whether resources apply.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=resources");
    let macros = [
        concat!("VERSION_MAJOR=", env!("CARGO_PKG_VERSION_MAJOR")),
        concat!("VERSION_MINOR=", env!("CARGO_PKG_VERSION_MINOR")),
        concat!("VERSION_PATCH=", env!("CARGO_PKG_VERSION_PATCH")),
        concat!("VERSION_STR=\"", env!("CARGO_PKG_VERSION"), "\""),
    ];
    embed_resource::compile("resources/typomend.rc", &macros)
        .manifest_required()
        .expect("compiling resources/typomend.rc");
}
