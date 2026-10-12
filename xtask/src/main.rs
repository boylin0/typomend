// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Development tasks, run as `cargo xtask <task>`.
//!
//! This follows the xtask convention (<https://github.com/matklad/cargo-xtask>):
//! project automation is a plain Rust program in the workspace, so it needs
//! nothing beyond the Rust toolchain. The git hooks in `.githooks/` call into
//! it, which keeps the checks a hook runs identical to the ones CI runs.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Help text, also the error shown for an unknown task.
const USAGE: &str = "\
usage: cargo xtask <task>

tasks:
  check           run every check CI runs: format, license headers, clippy, tests
  pre-commit      fast checks for the pre-commit hook: format, headers, clippy
  headers [--fix] check, or add, the copyright and license header of source files
  commit-msg FILE check that a commit message follows Conventional Commits
  install-hooks   point git at .githooks/ so the hooks run on every commit";

/// Header at the top of every source file, one line per entry: the copyright
/// line, the SPDX tag and the notice MPL-2.0 Exhibit A asks for. An empty entry
/// is a blank comment line.
///
/// MPL-2.0 section 3.4 forbids removing copyright and license notices from the
/// source, so these lines stay in every copy and modification of the file.
const HEADER: [&str; 6] = [
    "Copyright boylin0 and the Typomend contributors.",
    "SPDX-License-Identifier: MPL-2.0",
    "",
    "This Source Code Form is subject to the terms of the Mozilla Public",
    "License, v. 2.0. If a copy of the MPL was not distributed with this",
    "file, You can obtain one at https://mozilla.org/MPL/2.0/.",
];

/// The Exhibit A notice alone, which files carried before the copyright line
/// and the SPDX tag were added above it.
const NOTICE: &[&str] = HEADER.split_at(3).1;

/// Conventional Commits types accepted by `commit-msg`.
const COMMIT_TYPES: [&str; 11] = [
    "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test",
];

/// Runs the task named by the first argument and prints its error, if any.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check(true),
        Some("pre-commit") => check(false),
        Some("headers") => headers(args.iter().any(|a| a == "--fix")),
        Some("commit-msg") => match args.get(1) {
            Some(file) => commit_msg(Path::new(file)),
            None => Err("commit-msg needs the path of the message file".into()),
        },
        Some("install-hooks") => install_hooks(),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Result of a task, with the message to show the user as the error.
type Result<T = ()> = std::result::Result<T, String>;

/// Runs the format, header and clippy checks, and the tests when `full` is set.
///
/// The pre-commit hook skips the tests to stay fast.
fn check(full: bool) -> Result {
    step("cargo fmt", &["fmt", "--all", "--check"])
        .map_err(|e| format!("{e}\nrun `cargo fmt --all` to fix the formatting"))?;
    headers(false)?;
    step(
        "cargo clippy",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    if full {
        step("cargo test", &["test", "--workspace", "--locked"])?;
    }
    Ok(())
}

/// Runs `cargo` with `args` in the workspace root and fails if it does.
fn step(name: &str, args: &[&str]) -> Result {
    eprintln!("xtask: {name}");
    let status = Command::new(cargo())
        .args(args)
        .current_dir(root())
        .status()
        .map_err(|e| format!("running {name}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed"))
    }
}

/// Comment syntax per file extension: line prefix, or opening and closing
/// delimiters for formats without line comments.
fn comment_style(path: &Path) -> Option<(&'static str, &'static str)> {
    match path.extension()?.to_str()? {
        "rs" | "rc" | "h" | "cs" => Some(("// ", "")),
        "ps1" | "py" | "ftl" | "toml" | "yml" | "yaml" => Some(("# ", "")),
        "html" => Some(("<!-- ", " -->")),
        _ => None,
    }
}

/// Files whose first lines must carry the notice: every file git tracks or
/// would add, in a format with comments, except the generated `Cargo.lock`.
fn header_files() -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root())
        .output()
        .map_err(|e| format!("running git ls-files: {e}"))?;
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing
        .lines()
        .map(|line| root().join(line))
        .filter(|path| path.is_file() && comment_style(path).is_some())
        .filter(|path| path.file_name().is_none_or(|name| name != "Cargo.lock"))
        .collect())
}

/// Returns whether the first lines of `text` carry every line of the header.
fn has_header(text: &str) -> bool {
    let top: Vec<&str> = text.lines().take(10).collect();
    HEADER
        .iter()
        .filter(|line| !line.is_empty())
        .all(|line| top.iter().any(|t| t.contains(line)))
}

/// Formats `lines` as comments with the delimiters of `style`, ending each
/// with `eol`.
///
/// An empty entry becomes an empty line comment, or is left out where the
/// syntax only has block comments.
fn commented(lines: &[&str], (open, close): (&str, &str), eol: &str) -> String {
    lines
        .iter()
        .filter_map(|line| match (line.is_empty(), close.is_empty()) {
            (false, _) => Some(format!("{open}{line}{close}{eol}")),
            (true, true) => Some(format!("{}{eol}", open.trim_end())),
            (true, false) => None,
        })
        .collect()
}

/// Returns `text` with the header at the top, in place of the Exhibit A
/// notice alone when the file starts with it.
fn with_header(text: &str, style: (&str, &str)) -> String {
    // PowerShell scripts are checked out with CRLF endings.
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let header = commented(&HEADER, style, eol);
    match text.strip_prefix(&commented(NOTICE, style, eol)) {
        Some(rest) => format!("{header}{rest}"),
        None => format!("{header}{eol}{text}"),
    }
}

/// Fails when a source file lacks the header, or adds it when `fix` is set.
fn headers(fix: bool) -> Result {
    eprintln!("xtask: license headers");
    let mut missing = Vec::new();
    for path in header_files()? {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        if has_header(&text) {
            continue;
        }
        if fix {
            let style = comment_style(&path).unwrap_or(("// ", ""));
            std::fs::write(&path, with_header(&text, style))
                .map_err(|e| format!("writing {}: {e}", path.display()))?;
        }
        missing.push(path);
    }
    if missing.is_empty() {
        return Ok(());
    }
    let list: Vec<String> = missing
        .iter()
        .map(|p| format!("  {}", relative(p)))
        .collect();
    if fix {
        eprintln!("added the license header to:\n{}", list.join("\n"));
        Ok(())
    } else {
        Err(format!(
            "missing the license header:\n{}\nrun `cargo xtask headers --fix` to add it",
            list.join("\n")
        ))
    }
}

/// Checks the subject line of the commit message in `file`.
fn commit_msg(file: &Path) -> Result {
    let text =
        std::fs::read_to_string(file).map_err(|e| format!("reading {}: {e}", file.display()))?;
    let subject = text
        .lines()
        .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .unwrap_or_default();
    // Messages git writes itself.
    if ["Merge ", "Revert \"", "fixup! ", "squash! ", "amend! "]
        .iter()
        .any(|p| subject.starts_with(p))
    {
        return Ok(());
    }
    if is_conventional(subject) {
        return Ok(());
    }
    Err(format!(
        "commit message does not follow Conventional Commits (https://www.conventionalcommits.org/):\n\
         \n    {subject}\n\n\
         expected `<type>[(scope)][!]: <description>`, for example `fix(observer): handle empty selections`\n\
         types: {}",
        COMMIT_TYPES.join(", ")
    ))
}

/// Returns whether `subject` reads `<type>[(scope)][!]: <description>` with a
/// type from [`COMMIT_TYPES`].
fn is_conventional(subject: &str) -> bool {
    let Some((head, description)) = subject.split_once(": ") else {
        return false;
    };
    if description.trim().is_empty() {
        return false;
    }
    let head = head.strip_suffix('!').unwrap_or(head);
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => match rest.strip_suffix(')') {
            Some(scope) => (kind, Some(scope)),
            None => return false,
        },
        None => (head, None),
    };
    let scope_ok = scope.is_none_or(|s| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_/.".contains(c))
    });
    COMMIT_TYPES.contains(&kind) && scope_ok
}

/// Points this clone's `core.hooksPath` at `.githooks/`.
fn install_hooks() -> Result {
    let status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(root())
        .status()
        .map_err(|e| format!("running git config: {e}"))?;
    if !status.success() {
        return Err("git config core.hooksPath failed".into());
    }
    eprintln!("git hooks installed: pre-commit and commit-msg now run from .githooks/");
    Ok(())
}

/// Returns the cargo that started xtask, so the same toolchain runs the checks.
fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Returns the workspace root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

/// Formats `path` relative to the workspace root for messages.
fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_conventional_subjects() {
        for subject in [
            "feat: add rules file",
            "fix(observer): wait for commit",
            "feat(app)!: rename the config keys",
            "docs(readme/zh-TW): fix table",
        ] {
            assert!(is_conventional(subject), "{subject}");
        }
    }

    #[test]
    fn rejects_other_subjects() {
        for subject in [
            "add rules file",
            "feature: add rules",
            "fix(): empty scope",
            "fix(observer: unclosed",
            "fix: ",
            "Fix: capitalized type",
        ] {
            assert!(!is_conventional(subject), "{subject}");
        }
    }

    #[test]
    fn finds_the_header_near_the_top() {
        let header = commented(&HEADER, ("// ", ""), "\n");
        assert!(has_header(&format!("{header}\nfn main() {{}}")));
        assert!(!has_header("fn main() {}"));
        let notice = commented(NOTICE, ("// ", ""), "\n");
        assert!(!has_header(&format!("{notice}\nfn main() {{}}")));
    }

    #[test]
    fn adds_the_header_to_a_file_without_one() {
        let fixed = with_header("fn main() {}\n", ("// ", ""));
        assert!(fixed.starts_with("// Copyright boylin0"));
        assert!(fixed.contains("MPL-2.0\n//\n// This Source Code Form"));
        assert!(fixed.ends_with("MPL/2.0/.\n\nfn main() {}\n"));
    }

    #[test]
    fn replaces_the_notice_alone_with_the_header() {
        let notice = commented(NOTICE, ("# ", ""), "\r\n");
        let fixed = with_header(&format!("{notice}\r\nparam()\r\n"), ("# ", ""));
        assert_eq!(
            fixed,
            format!("{}\r\nparam()\r\n", commented(&HEADER, ("# ", ""), "\r\n"))
        );
        assert_eq!(fixed.matches("This Source Code Form").count(), 1);
    }

    #[test]
    fn leaves_out_empty_block_comments() {
        let header = commented(&HEADER, ("<!-- ", " -->"), "\n");
        assert_eq!(header.lines().count(), 5);
        assert!(!header.contains("<!--  -->"));
    }
}
