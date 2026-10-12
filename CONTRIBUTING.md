# Contributing to Typomend

Thanks for your interest. Typomend is a side project, so replies may take a while.

Everyone taking part in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- For anything larger than a small fix, open an issue first so we can agree on the approach.
- Typomend must never modify, replace or reconfigure the user's input method, and must never send typed text anywhere. Changes that break either rule will not be accepted.

## Development

Building needs Rust 1.90 or later. On Windows 10 or 11, use the MSVC toolchain:

```powershell
cargo build --release
.\target\release\typomend.exe
```

### Platforms

`typomend` and `typomend-observer` are Windows-only. On other systems they compile to a stub or an empty crate. `typomend-engine`, `typomend-scorer` and `xtask` build and test on Windows, Linux and macOS, and CI runs `cargo xtask check` on all three, plus an MSRV job and cargo-deny.

On Linux or macOS:

```sh
cargo xtask check
cargo test -p typomend-engine -p typomend-scorer
```

On Linux, the scorer's HTTPS support needs `libssl-dev` and `pkg-config`:

```sh
sudo apt-get install libssl-dev pkg-config
```

### Optional tools

- Python 3 runs `tools/build-scorer-data.py`, which regenerates the scorer's data files.
- The .NET SDK builds `tools/UiaProbe`.

Neither is needed to build or test Typomend.

### Git hooks and tasks

After cloning, install the git hooks once:

```powershell
cargo xtask install-hooks
```

Project automation follows the [xtask](https://github.com/matklad/cargo-xtask) convention: it is a Rust program in `xtask/`, run with `cargo xtask <task>`, and needs nothing beyond the Rust toolchain.

| Task | What it does |
|------|--------------|
| `cargo xtask check` | Everything CI checks: formatting, license headers, clippy, tests |
| `cargo xtask headers --fix` | Adds the copyright and license header to source files that lack it |
| `cargo fmt --all` | Formats the code |

### What is enforced

| Check | Tool | When |
|-------|------|------|
| Formatting | rustfmt, configured in `rustfmt.toml` | pre-commit hook, CI |
| Copyright and license header at the top of every source file, Python files included | `cargo xtask headers` | pre-commit hook, CI |
| Lints, as errors | clippy, configured under `[workspace.lints]` in `Cargo.toml` | pre-commit hook, CI |
| Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) | `cargo xtask commit-msg` | commit-msg hook |
| Tests | `cargo test` | CI |
| Dependency licenses, security advisories, crate sources | [cargo-deny](https://github.com/EmbarkStudios/cargo-deny), configured in `deny.toml` | CI |

Every `unsafe` block needs a `// SAFETY:` comment explaining why it is sound, every public item of the library crates needs a doc comment, and `unwrap()` is not allowed outside tests. Comments follow the style of the Rust standard library: a doc comment opens with a one-sentence summary in the third person, such as "Returns the rule list.", and a plain comment explains why the code does something, not what it does.

### License headers

Typomend is licensed under MPL-2.0, which applies file by file. Every source file starts with this header, in the file's comment syntax:

```rust
// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
```

`cargo xtask headers --fix` adds it to new files, Python files included, and adds the first two lines to files that carry only the notice below them.

**Code or data taken from another project keeps its own copyright and license notice instead of this header.** List it under third-party components in [NOTICE.md](NOTICE.md), as the files under `crates/typomend-scorer/data/` generated from Unihan and OpenCC are, and exclude it in `header_files` in `xtask/src/main.rs` when it is in a format the check covers.

### Watching the observer

To watch the observer without the tray app:

```powershell
$env:RUST_LOG = "typomend_observer=debug"
cargo run -p typomend-observer --example console
```

Arguments such as `windowsterminal=terminal` set the correction mode of a program, as the **Apps** tab does.

### End-to-end tests

`tools/Send-TestInput.ps1` types into a real text field through Microsoft Bopomofo and `tools/test-page.html` provides browser fields to type into. `tools/Test-Terminal.ps1` opens a PowerShell prompt in Windows Terminal or the classic console, types a command with a typo and checks that it was corrected; run the console example with `windowsterminal=terminal conhost=terminal` first. **These scripts take over the foreground window and the keyboard**; run them only when nobody is using the machine. Each script lists its parameters at the top. `-Window` is the top-level window handle in decimal; add `-AutomationId` for a field inside a browser page:

```powershell
.\tools\Send-TestInput.ps1 -Mode bopomofo -Window 9766470 -AutomationId ta
.\tools\Send-TestInput.ps1 -Mode bopomofo -Window 15339228
```

`Send-TestInput.ps1` clears the target field first. `tools/UiaProbe` lists the UI Automation patterns a control supports; with `--events` it prints TextEdit and TextChanged events as they arrive.

## Translations

User-facing strings live in Fluent files under `crates/typomend/i18n/<language>/typomend.ftl`. To add a language, copy `en/typomend.ftl`, translate it, and add the language to `Language` in `crates/typomend/src/i18n.rs`. Message ids are checked at compile time.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org/), for example `fix(observer): ...`.
- Keep pull requests focused, and add a line to `CHANGELOG.md` under **Unreleased** for user-visible changes. The pull request template lists what to check.
- Report security vulnerabilities privately, as described in [SECURITY.md](SECURITY.md), not in a public issue.
