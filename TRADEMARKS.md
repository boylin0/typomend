# Trademark policy

This policy covers the Typomend name and logo: what you can do with them without asking, and what a product built from Typomend needs to change. It is separate from the copyright license. MPL 2.0 lets you use, modify and sell the code, and its section 2.3 states that it grants no rights in the trademarks or logos of any contributor.

## Quick reference

| You want to | Allowed? |
|-------------|----------|
| Say your product is based on, forked from or compatible with Typomend | Yes |
| Keep "Typomend" in source code, file names, crate names, and copyright and license notices | Yes |
| Redistribute an official release unmodified, under the Typomend name | Yes |
| Write about, review, teach or link to Typomend, with or without its logo | Yes |
| Distribute a modified version, including a paid or competing one | Yes, under your own name and logo |
| Call a modified version Typomend, or a name that could be mistaken for it | No |
| Use the Typomend logo or icons as the logo or icon of your product | No |
| Claim or suggest that your product is official, or endorsed or sponsored by the Typomend project | No |
| Use Typomend in a domain name, app store listing, account name or company name | Ask first |

## Which marks does this cover?

- The name **Typomend**, and names that could be mistaken for it, such as Typomend Pro, Typomend+ or TypoMend Lite.
- The Typomend logo and lockups in `.github/assets/`, and the program and tray icons in `crates/typomend/resources/`.

boylin0 maintains these marks for the official project: the repository at <https://github.com/boylin0/typomend>, its mirror at <https://github.com/typomend/typomend> and the website <https://typomend.github.io>. Releases published in that repository are the official Typomend releases.

The logo files are in the repository, so you may copy them as part of the source code. Using them to identify your own product is what this policy restricts.

## What does a modified version need to change?

A modified version is any build from changed source code. Before you distribute one:

1. Give it a name of your own everywhere a user sees it: window titles, the tray tooltip, the About tab, the executable's file properties and the release archive.
2. Replace the logo and the icons.
3. Keep the copyright and license notices in every file that came from Typomend. MPL 2.0 section 3.4 requires this, and naming Typomend there never conflicts with this policy.

In this codebase, the name and the logo appear here:

| What | Where |
|------|-------|
| User interface text | `crates/typomend/i18n/<language>/typomend.ftl` |
| Window title and the Start with Windows entry | `crates/typomend/src/platform.rs` |
| File properties and dialog templates | `crates/typomend/resources/typomend.rc` |
| Program and tray icons | `crates/typomend/resources/*.ico` |
| Logo and lockups for the readme | `.github/assets/` |

Also changing the window class in `crates/typomend/src/app.rs`, the settings folder in `crates/typomend/src/config.rs` and the log folder in `crates/typomend/src/logging.rs` keeps your product from clashing with an official copy on the same computer. This policy does not require it.

## Which builds may keep the name?

A build of unmodified source, or an official release packaged for a package manager such as winget or Scoop, may keep the Typomend name and logo, **as long as it does not present itself as an official release**. Say who built or packaged it.

## What does this policy not do?

- **It adds no condition to MPL 2.0.** Everything the license lets you do with the code, including selling a competing product, stays allowed under a name and logo of your own.
- It does not limit uses that the law allows without permission, such as naming Typomend to describe your product truthfully.
- It does not make the attribution suggested in [NOTICE.md](NOTICE.md) mandatory.

## How do I ask for permission?

Open an issue at <https://github.com/boylin0/typomend/issues> describing the use you have in mind.
