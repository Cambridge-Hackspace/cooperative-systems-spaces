//! Every `npm run <script>` in a workflow must name a script that exists.
//!
//! CI called `npm run type-check` in `frontend_edge`, and that script had never
//! existed there. The job failed with
//!
//! ```text
//! npm error Missing script: "type-check"
//! ```
//!
//! after `npm ci` had installed the whole dependency tree — three minutes spent
//! to discover a typo-class mistake that is visible in two files side by side.
//!
//! The mistake underneath it was worse than a typo. `frontend/` was given
//! eslint, prettier, a type-check and a strictness ratchet; `frontend_edge/` was
//! given none of them, and the CI job for it was written as though it had. So
//! the workflow encoded an intention rather than a fact, and because the
//! workflow had never run, nothing contradicted it.
//!
//! This check is the cheap half of that lesson: a script named in CI and absent
//! from `package.json` is caught here in milliseconds, on any host, with no
//! `node_modules` and no network. The expensive half — actually giving the
//! second frontend the tooling — is work, and this test is what stops the
//! workflow from claiming it is done before it is.
//!
//! What this does not prove: that the script does anything useful, or that it
//! passes. `"lint": "true"` would satisfy every assertion here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use css_checks::repo_root;

fn workflow_files() -> Vec<PathBuf> {
    let dir = repo_root().join(".github/workflows");
    let entries = std::fs::read_dir(&dir)
        .expect(".github/workflows must exist and be readable from the repo root");
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    out.sort();
    out
}

/// The script names in a `package.json`, read without a JSON parser.
///
/// `checks` depends on nothing, deliberately, and the shape being read here is
/// written by npm rather than by hand: `"scripts"`, then one `"name": "cmd"`
/// per line, then a closing brace. If that ever stops being true this returns
/// too few names and the test fails loudly, which is the right direction to be
/// wrong in.
fn scripts_in(manifest: &Path) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let mut names = BTreeSet::new();
    let mut inside = false;

    for line in text.lines() {
        let t = line.trim();
        if !inside {
            if t.starts_with("\"scripts\"") {
                inside = true;
            }
            continue;
        }
        if t == "}" || t == "}," {
            break;
        }
        // `"name": "command",`
        if let Some(rest) = t.strip_prefix('"') {
            if let Some((name, _)) = rest.split_once('"') {
                names.insert(name.to_string());
            }
        }
    }

    inside.then_some(names)
}

/// `(workflow, line, working_directory, script)` for every `npm run` in CI.
fn npm_run_invocations(text: &str) -> Vec<(usize, Option<String>, String)> {
    let mut out = Vec::new();
    let mut working_dir: Option<String> = None;

    for (i, line) in text.lines().enumerate() {
        let t = line.trim();

        if t.starts_with('#') {
            continue;
        }

        // A new step resets the directory. Without this a `working-directory:`
        // on one step would leak onto every later step in the job, and the
        // check would look for `frontend_edge`'s scripts in `frontend`.
        if t.starts_with("- ") {
            working_dir = None;
        }

        let body = t.trim_start_matches("- ").trim();
        if let Some(dir) = body.strip_prefix("working-directory:") {
            working_dir = Some(dir.trim().trim_matches('"').trim_matches('\'').to_string());
            continue;
        }

        // Both `run: npm run x` and a `run: |` block whose lines invoke npm.
        if let Some(idx) = t.find("npm run ") {
            let after = &t[idx + "npm run ".len()..];
            let script = after
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches('"')
                .trim_matches('\'');
            if !script.is_empty() && !script.starts_with('-') {
                out.push((i + 1, working_dir.clone(), script.to_string()));
            }
        }
    }

    out
}

#[test]
fn every_npm_script_ci_calls_actually_exists() {
    let mut failures = Vec::new();
    let mut checked = 0usize;

    for path in workflow_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            failures.push(format!("{} could not be read", path.display()));
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        for (line, dir, script) in npm_run_invocations(&text) {
            checked += 1;

            let dir = dir.unwrap_or_default();
            let manifest = repo_root().join(&dir).join("package.json");

            let Some(scripts) = scripts_in(&manifest) else {
                failures.push(format!(
                    "{name}:{line}  runs `npm run {script}` in `{dir}`, which has no \
                     readable package.json with a \"scripts\" block ({})",
                    manifest.display()
                ));
                continue;
            };

            if !scripts.contains(&script) {
                let known: Vec<&str> = scripts.iter().map(String::as_str).collect();
                failures.push(format!(
                    "{name}:{line}  runs `npm run {script}` in `{dir}`, which defines \
                     no such script. It has: {}",
                    known.join(", ")
                ));
            }
        }
    }

    // A check that finds nothing to check passes, and looks exactly like a
    // check that found nothing wrong.
    assert!(
        checked > 0,
        "no `npm run` invocations found in any workflow. This check reads them by \
         line, so a reformat or a move turns it into a permanent silent pass."
    );

    assert!(
        failures.is_empty(),
        "CI calls an npm script that does not exist. The job installs the whole \
         dependency tree first, so this costs minutes to discover there and \
         milliseconds to discover here.\n\n{}",
        failures.join("\n")
    );
}
