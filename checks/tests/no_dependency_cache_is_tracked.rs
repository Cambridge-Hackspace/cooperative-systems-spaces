//! No dependency directory or tool cache is tracked in git.
//!
//! `node_modules/.vite/vitest/<hash>/results.json` was tracked for a month
//! (#195). It arrived with #87 because the repository root is not a Node
//! project and nothing ignored a root `node_modules` -- only
//! `/frontend/node_modules` and `/frontend_edge/node_modules` -- so a vitest
//! run from the wrong directory wrote a cache there and a `git add` took it.
//!
//! The cost is not a noisy `git status`. A tracked file that every other
//! checkout lacks appears as a *deletion* in every worktree, which is how this
//! was found: a stray cache directory read as a staged delete of a file nobody
//! had written. And `run.sh`'s preflight has an `artifacts/commit-matches-tree`
//! case whose whole job is to stop a battery testing yesterday's binaries --
//! a tree that is permanently dirty or permanently mid-deletion makes that
//! check permanently uninformative.
//!
//! Two oracles, because either alone can pass while the defect returns:
//! nothing of this shape is tracked *now*, and the rule that keeps it that way
//! is still in `.gitignore`. Removing the rule is what let the first one in.
//!
//! What this does NOT prove: that `.gitignore` is otherwise correct, or that a
//! cache under a name not listed here is caught. The list is deliberately
//! short and literal -- a directory a package manager or test runner owns --
//! rather than a guess at everything generated.
use css_checks::repo_root;
use std::process::Command;

/// Path fragments that mean "a tool owns this directory, not a person".
///
/// Each is matched as a path segment, so `node_modules/` catches it at any
/// depth while a file merely *named* something similar is left alone.
const TOOL_OWNED: &[&str] = &[
    "node_modules",
    ".vite",
    ".vitest",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
];

/// Every tracked path that lies under a tool-owned directory.
///
/// A function over a list rather than assertions inline, so the oracle can be
/// driven against the path that shipped -- see the self-tests. A check never
/// observed complaining is a check whose value is unmeasured.
fn offenders(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| {
            p.split('/')
                .any(|segment| TOOL_OWNED.iter().any(|owned| segment == *owned))
        })
        .cloned()
        .collect()
}

fn tracked_files() -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["ls-files", "-z"])
        .output()
        .expect("git ls-files runs");
    assert!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn no_tool_owned_directory_is_tracked() {
    let tracked = tracked_files();
    // Anti-vacuity: an empty or tiny list passes the assertion below for the
    // wrong reason -- a broken command, or a run outside a checkout.
    assert!(
        tracked.len() > 500,
        "git ls-files reported only {} tracked files; this is not reading the \
         repository, so the assertion below proves nothing",
        tracked.len()
    );

    let found = offenders(&tracked);
    assert!(
        found.is_empty(),
        "these tracked files live in a directory a tool owns: {found:?}\n\n\
         A dependency tree or a test-runner cache is not source. Untrack it \
         (`git rm --cached`) and make sure .gitignore covers the directory, or \
         the next run from the wrong working directory commits another one."
    );
}

#[test]
fn the_rule_that_keeps_them_out_is_still_there() {
    // The other side of the same claim. #195 was possible because the root
    // `node_modules` had no rule at all, so "nothing is tracked today" would
    // have passed on the day before it was committed.
    for dir in [
        "node_modules/",
        "frontend/node_modules/",
        "frontend_edge/node_modules/",
        "target/",
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo_root())
            .args(["check-ignore", "-q", dir])
            .status()
            .expect("git check-ignore runs");
        assert!(
            status.success(),
            "{dir} is not gitignored. A run that writes there can commit its \
             output, which is exactly how #195 happened."
        );
    }
}

#[cfg(test)]
mod the_check_rejects_what_it_is_for {
    use super::*;

    /// The path that actually shipped.
    #[test]
    fn the_file_from_195_is_caught() {
        let shipped =
            "node_modules/.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json"
                .to_string();
        let found = offenders(&[shipped.clone()]);
        assert_eq!(
            found,
            vec![shipped],
            "the oracle does not complain about the file it was written for"
        );
    }

    /// And a nested one, since the segment match is the whole mechanism.
    #[test]
    fn a_cache_nested_anywhere_is_caught() {
        let paths = vec![
            "frontend/node_modules/x/y.js".to_string(),
            "tools/scripts/__pycache__/helper.cpython-311.pyc".to_string(),
        ];
        assert_eq!(offenders(&paths).len(), 2, "{paths:?}");
    }

    /// The control: real source, including names that merely resemble the
    /// banned ones, must come back clean. Without this, a match on substrings
    /// rather than segments would read as the oracle working.
    #[test]
    fn source_is_left_alone() {
        let paths = vec![
            "server/src/main.rs".to_string(),
            "frontend/src/components/AuditLogTable.vue".to_string(),
            "e2e/drivers/lease.mjs".to_string(),
            // Not a tool-owned directory: a file whose NAME contains one.
            "docs/node_modules_policy.md".to_string(),
            "frontend/src/vite-env.d.ts".to_string(),
            "checks/tests/no_dependency_cache_is_tracked.rs".to_string(),
        ];
        assert!(
            offenders(&paths).is_empty(),
            "the oracle complains about source: {:?}",
            offenders(&paths)
        );
    }
}
