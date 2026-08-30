//! Everything `e2e/build.sh` writes into the tree is gitignored.
//!
//! `build.sh` puts its caches at `${REAPER_CACHE_*:-${ROOT}/e2e/...}`. Inside a
//! reaper session those variables are set and the fallbacks never fire; in CI,
//! and for anyone running the build verb by hand, they always do. So the
//! fallback paths are the ones nobody exercises and nobody notices going
//! untracked.
//!
//! `e2e/.cargo` was exactly that: a cargo registry, hundreds of megabytes,
//! written by every non-reaper run and ignored by no rule. The cost is not
//! merely a noisy `git status`. `run.sh`'s preflight has an
//! `artifacts/commit-matches-tree` case whose entire job is to stop a run
//! testing yesterday's binaries, and a permanently dirty tree is a
//! permanently uninformative answer from it.
//!
//! What this does NOT prove: that the ignore rules are otherwise correct, or
//! that a path outside these two patterns is covered. It reads the fallbacks
//! literally out of the script, so a cache added with a different shape is
//! invisible here -- `every_fallback_was_actually_found` is what keeps that
//! from being silent.

use css_checks::{read, repo_root};
use std::process::Command;

/// Every `${SOMETHING:-${ROOT}/path}` default in `e2e/build.sh`, as the path.
fn fallback_paths() -> Vec<String> {
    let source = read("e2e/build.sh");
    let mut found = Vec::new();

    for line in source.lines() {
        let mut rest = line;
        while let Some(i) = rest.find(":-${ROOT}/") {
            let after = &rest[i + ":-${ROOT}/".len()..];
            let end = after
                .find(|c: char| c == '}' || c == '"' || c == '\'' || c.is_whitespace())
                .unwrap_or(after.len());
            let path = &after[..end];
            if !path.is_empty() {
                found.push(path.to_string());
            }
            rest = &after[end..];
        }
    }

    found.sort();
    found.dedup();
    found
}

#[test]
fn every_fallback_was_actually_found() {
    // Anti-vacuity, and the thing that stops this file quietly covering less
    // than it did. An empty list passes every assertion below.
    let found = fallback_paths();
    assert!(
        found.len() >= 3,
        "parsed only {found:?} out of e2e/build.sh. The script sets its caches \
         with `${{VAR:-${{ROOT}}/path}}`; if that shape changed, this check is \
         reading nothing and the paths it is supposed to cover are unwatched."
    );
    assert!(
        found.iter().any(|p| p.contains("cargo")),
        "the CARGO_HOME fallback is missing from {found:?}, which is the one \
         that prompted this file"
    );
}

#[test]
fn every_build_output_path_is_gitignored() {
    let root = repo_root();
    let mut unignored = Vec::new();

    for path in fallback_paths() {
        // `check-ignore` cannot tell a non-existent path is a directory, and
        // every rule here is written with a trailing slash, so ask about the
        // directory form.
        let probe = format!("{path}/");
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .arg("check-ignore")
            .arg("-q")
            .arg(&probe)
            .status()
            .expect("git check-ignore must be runnable");

        if !status.success() {
            unignored.push(probe);
        }
    }

    assert!(
        unignored.is_empty(),
        "e2e/build.sh writes these into the tree and .gitignore does not cover \
         them:\n{unignored:?}\n\n\
         Inside a reaper session the REAPER_CACHE_* variables are set and these \
         fallbacks never fire, so this goes unnoticed until CI or a hand-run \
         build fills the working tree with a cargo registry or an npm cache. \
         Add the directory (with a trailing slash) to .gitignore."
    );
}
