//! reaper must not be weaker than CI.
//!
//! `reaper` is the pre-push loop and CI is the gate, and the whole value of the
//! first is that it rejects what the second would. A step that exists only in
//! CI is a step you discover by pushing, watching a job fail twenty minutes
//! later, committing a fix, and pushing again -- which puts the fix-the-fix
//! commits in the permanent history of a branch.
//!
//! That is not hypothetical here. Two such fixups are in this branch's history
//! from the tier-2 sweep: `npm run lint` and `npm run type-check` were failing
//! on committed spec files, and nothing local was running them.
//!
//! So: every npm script CI runs on a frontend, and every cargo and shell gate
//! CI runs, must also appear in `e2e/build.sh`. The reverse is not required --
//! reaper may do more, and does (it builds both edge binaries and takes a
//! snapshot).
//!
//! What this does NOT prove: that the two run them with the same arguments, or
//! in the same environment, or that `e2e/build.sh` reaches the line. It is a
//! parity check on *what is invoked*, which is the failure that has actually
//! happened. Tier 6 running green in both places is the behavioural half.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

fn workflow_text() -> String {
    let dir = repo_root().join(".github/workflows");
    let mut out = String::new();
    for entry in std::fs::read_dir(&dir).expect(".github/workflows must be readable") {
        let path = entry.expect("readable").path();
        if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            out.push_str(&std::fs::read_to_string(&path).expect("workflow readable"));
            out.push('\n');
        }
    }
    out
}

/// Every `npm run <script>` in a body of text, ignoring comment lines.
fn npm_scripts(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let code = line.trim_start();
        if code.starts_with('#') {
            continue;
        }
        let mut rest = code;
        while let Some(i) = rest.find("npm run ") {
            rest = &rest[i + "npm run ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == ':' || *c == '_')
                .collect();
            if !name.is_empty() {
                out.insert(name);
            }
        }
    }
    out
}

#[test]
fn every_npm_script_ci_runs_is_also_run_before_a_push() {
    let ci = npm_scripts(&workflow_text());
    let build = npm_scripts(&read("e2e/build.sh"));

    // Anti-vacuity, both sides. A parser that stopped matching would otherwise
    // make this pass by finding nothing on either side.
    assert!(
        ci.len() >= 5,
        "only {} npm scripts found in the workflows; the parser stopped working \
         rather than CI getting smaller. Found: {ci:?}",
        ci.len()
    );
    assert!(
        build.len() >= 5,
        "only {} npm scripts found in e2e/build.sh; the parser stopped working. \
         Found: {build:?}",
        build.len()
    );

    let missing: Vec<&String> = ci.difference(&build).collect();
    assert!(
        missing.is_empty(),
        "CI runs these npm scripts and e2e/build.sh does not: {missing:?}\n\n\
         reaper is the pre-push loop; a gate that only exists in CI is one you \
         find by pushing, failing, fixing and pushing again. Add it to \
         `frontend_gate` in e2e/build.sh, or -- if it genuinely cannot run \
         there -- say why in a comment beside the CI step and add it to the \
         exemption list in this file.\n\n\
         CI runs:    {ci:?}\n\
         build runs: {build:?}"
    );
}

#[test]
fn the_shell_formatter_is_pinned_to_one_version_everywhere() {
    // A formatter is only "total and non-negotiable" if every machine runs the
    // same one. Debian bookworm ships shfmt 3.6.0, whose `-s` rewrites
    // `${VAR:-}` to `${VAR-}`; those differ for a set-but-empty variable and
    // later versions stopped doing it. Left unpinned, the container demanded a
    // rewrite that CI and this workstation both reject.
    let build = read("e2e/build.sh");
    let ci = workflow_text();

    let pinned = build
        .lines()
        .find_map(|l| l.trim().strip_prefix("SHFMT_VERSION=\"v"))
        .and_then(|v| v.strip_suffix('"'))
        .map(str::to_owned)
        .expect("e2e/build.sh must pin SHFMT_VERSION as \"vX.Y.Z\"");

    assert!(
        ci.contains(&format!("version: '{pinned}'")),
        "e2e/build.sh pins shfmt {pinned} and no workflow pins the same version. \
         An unpinned `setup-shfmt` installs whatever is latest, so the two drift \
         and the tree becomes unformattable to one of them."
    );

    assert!(
        !build.contains("shellcheck shfmt"),
        "shfmt is being installed from the distribution again. That is what \
         produced the 3.6.0 disagreement; it is bootstrapped with a checksum \
         below instead."
    );
}

#[test]
fn the_shell_linter_is_pinned_to_one_version_everywhere() {
    // Same reasoning as the formatter above, and the same failure: a rule one
    // version reports and another does not means the tree is clean on one
    // machine and dirty on the next. Debian bookworm packages 0.9.0, the
    // GitHub runners carry another, and the FreeBSD workstation has 0.11.0.
    let build = read("e2e/build.sh");
    let ci = workflow_text();

    let pinned = build
        .lines()
        .find_map(|l| l.trim().strip_prefix("SHELLCHECK_VERSION=\"v"))
        .and_then(|v| v.strip_suffix('"'))
        .map(str::to_owned)
        .expect("e2e/build.sh must pin SHELLCHECK_VERSION as \"vX.Y.Z\"");

    assert!(
        ci.contains(&format!("ver=v{pinned}")),
        "e2e/build.sh pins shellcheck {pinned} and no workflow pins the same \
         version. Left to the distribution the two drift, and the tree becomes \
         un-lintable to one of them."
    );

    assert!(
        !ci.contains("install -y --no-install-recommends shellcheck"),
        "shellcheck is being installed from the distribution again in CI. That \
         is the thing this pin exists to stop."
    );
}

/// Non-npm gates CI runs, and the substring that proves `e2e/build.sh` runs it.
///
/// Written out rather than parsed, because a `cargo` invocation has too many
/// shapes to scrape reliably and a scraper that silently matched nothing would
/// be worse than a short list somebody has to maintain on purpose.
const GATES: &[(&str, &str)] = &[
    ("cargo fmt --all -- --check", "cargo fmt --all -- --check"),
    (
        "cargo test --locked --all-targets",
        "cargo test --locked --all-targets",
    ),
    ("cargo test --locked --doc", "cargo test --locked --doc"),
    (
        "cargo check --locked --workspace --all-targets",
        "cargo check --locked --workspace --all-targets",
    ),
    ("./e2e/lint.sh", "./e2e/lint.sh"),
];

#[test]
fn every_other_gate_ci_runs_is_also_run_before_a_push() {
    let ci = workflow_text();
    let build = read("e2e/build.sh");

    let mut missing_from_ci = Vec::new();
    let mut missing_from_build = Vec::new();

    for (in_ci, in_build) in GATES {
        // If CI stopped running it, this entry is stale and the list is lying
        // about what the gate is. That is worth failing on too.
        if !ci.contains(in_ci) {
            missing_from_ci.push(*in_ci);
        }
        if !build.contains(in_build) {
            missing_from_build.push(*in_build);
        }
    }

    assert!(
        missing_from_ci.is_empty(),
        "these are listed here as CI gates but no workflow runs them any more: \
         {missing_from_ci:?}. If a gate was dropped from CI deliberately, drop \
         it from this list and say why; if it was renamed, update the list."
    );
    assert!(
        missing_from_build.is_empty(),
        "CI runs these and e2e/build.sh does not: {missing_from_build:?}\n\n\
         reaper is the pre-push loop. Anything CI can reject has to be \
         rejectable here first, or the loop is push-fail-fix-push."
    );
}
