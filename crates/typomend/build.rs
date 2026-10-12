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
//! common controls and per-monitor DPI awareness that it declares. The
//! resource compiler embeds a manifest without expanding macros in it, so the
//! manifest is written from a template with the version already filled in.

use std::path::Path;

/// Marks where the manifest template takes the version.
const VERSION_PLACEHOLDER: &str = "@VERSION@";

fn main() {
    // Build scripts run on the host, so the target decides whether resources apply.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=resources");
    write_manifest();
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

/// Writes `typomend.manifest` into `OUT_DIR` from
/// `resources/typomend.manifest.in`, with the crate version filled in.
///
/// embed-resource puts `OUT_DIR` on the include path of the resource
/// compiler, which is where `typomend.rc` finds the file.
fn write_manifest() {
    let template = std::fs::read_to_string("resources/typomend.manifest.in")
        .expect("reading resources/typomend.manifest.in");
    assert!(
        template.contains(VERSION_PLACEHOLDER),
        "resources/typomend.manifest.in lacks {VERSION_PLACEHOLDER}"
    );
    // A manifest version has four parts. The last is 0, as in FILEVERSION.
    let version = concat!(
        env!("CARGO_PKG_VERSION_MAJOR"),
        ".",
        env!("CARGO_PKG_VERSION_MINOR"),
        ".",
        env!("CARGO_PKG_VERSION_PATCH"),
        ".0"
    );
    let out_dir = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR for build scripts");
    std::fs::write(
        Path::new(&out_dir).join("typomend.manifest"),
        template.replace(VERSION_PLACEHOLDER, version),
    )
    .expect("writing typomend.manifest");
}
