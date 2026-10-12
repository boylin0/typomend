// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Downloads a model the way the settings window does.
//!
//! ```text
//! cargo run --release -p typomend-scorer --example download -- <id> <dir>
//! ```
//!
//! `<id>` is a model from `typomend_scorer::MODELS`. Set `STOP_AT_MB` to
//! cancel the download after that many megabytes, then run it again to see
//! it resume.

use anyhow::{Context, Result};
use typomend_scorer::MODELS;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let id = args.next().context("usage: download <id> <dir>")?;
    let dir = args.next().context("usage: download <id> <dir>")?;
    let model = MODELS
        .iter()
        .find(|m| m.id == id)
        .with_context(|| format!("unknown model {id}"))?;
    let stop_at: Option<u64> = std::env::var("STOP_AT_MB")
        .ok()
        .and_then(|v| v.parse().ok());
    let mut last = 0;
    typomend_scorer::download::download(model, dir.as_ref(), &mut |p| {
        let mb = p.done >> 20;
        if mb >= last + 25 {
            last = mb;
            println!(
                "{mb} / {} MB  {:.1} MB/s  {} threads",
                p.total.unwrap_or(0) >> 20,
                p.speed / 1048576.0,
                p.threads
            );
        }
        stop_at.is_none_or(|s| mb < s)
    })?;
    println!("done");
    Ok(())
}
