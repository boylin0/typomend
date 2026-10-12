# Notice

This file records where Typomend comes from, who wrote it and under which licenses its parts are distributed. Keeping it in copies and derivative works keeps that record with the code.

## Typomend

| Item | Value |
|------|-------|
| Project | Typomend |
| Original author | boylin0 |
| Copyright | Copyright boylin0 and the Typomend contributors |
| Source code | <https://github.com/boylin0/typomend> |
| Website | <https://typomend.github.io> |
| License | [Mozilla Public License 2.0](LICENSE), SPDX identifier `MPL-2.0` |

The Typomend contributors are everyone whose contribution is in the project's git history. Each contributor keeps the copyright in their contribution and licenses it under MPL 2.0.

## Does this file add any conditions?

No. MPL 2.0 has no clause that makes a notice file binding, unlike section 4(d) of the Apache License 2.0. What binds is [LICENSE](LICENSE) and the header at the top of every source file:

```rust
// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
```

**MPL 2.0 section 3.4 forbids removing or altering the substance of these notices**, so they stay in every copy and modification of the file. The attribution below is a request on top of that, not a condition of the license.

## How should a derivative work credit Typomend?

A product built from Typomend can credit it in its documentation, its release notes and its About page with these lines:

```text
Based on Typomend
Originally developed by boylin0
https://github.com/boylin0/typomend
Licensed under MPL 2.0
```

The same lines in Traditional Chinese:

```text
基於 Typomend 開發
原作者：boylin0
https://github.com/boylin0/typomend
採用 MPL 2.0 授權
```

An About page that also links to your own source code covers both what is asked and what is required:

```text
YourApp 1.2.0
Copyright 2027 Your Name
Source code: https://example.com/yourapp            <- required for MPL files you ship
Based on Typomend, originally developed by boylin0   <- requested
https://github.com/boylin0/typomend
Licensed under MPL 2.0
```

**Telling recipients of an executable where to get the source of its MPL-covered files is required** by MPL 2.0 section 3.2; an About page link is one way to do it. The "Based on Typomend" lines are not required, and neither this file nor [TRADEMARKS.md](TRADEMARKS.md) can make them so. Using the Typomend name this way is allowed by the trademark policy.

In this codebase the About tab is the `IDD_ABOUT` template in `crates/typomend/resources/typomend.rc`, filled in by `crates/typomend/src/dialog/pages.rs` from the `about-*` messages in `crates/typomend/i18n/<language>/typomend.ftl`.

## Which parts are under other licenses?

MPL 2.0 covers Typomend's own files. The components below keep their own licenses and copyright notices; MPL 2.0 does not relicense them, and nothing in this file limits the rights their licenses grant.

| Component | Where it is | License |
|-----------|-------------|---------|
| Rust crates Typomend depends on | Listed in `Cargo.lock` and compiled into `typomend.exe` | Each crate's own license; `deny.toml` lists the ones allowed, all of which can be combined with MPL 2.0 |
| Mandarin readings | `crates/typomend-scorer/data/readings.txt`, generated from the [Unicode Unihan database](https://www.unicode.org/charts/unihan.html) | Unicode License v3 |
| Traditional to Simplified character table | `crates/typomend-scorer/data/t2s.txt`, generated from [OpenCC](https://github.com/BYVoid/OpenCC) `TSCharacters.txt` | Apache-2.0 |
| Masked language models | Downloaded from Hugging Face only when you choose a model; not part of the source or the release archive | Each model's own license, shown next to it on the **Masked LM** tab |

## Trademarks

The Typomend name and logo are not licensed under MPL 2.0, whose section 2.3 grants no trademark rights. [TRADEMARKS.md](TRADEMARKS.md) describes how they may be used.
