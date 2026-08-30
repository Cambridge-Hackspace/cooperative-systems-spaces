//! Every job in every workflow must name a runner an arbitrary clone can get.
//!
//! This exists because the workflow spent two and a half months naming
//! `arc-runner-set` — a self-hosted Actions Runner Controller set registered to
//! the upstream organization — and self-hosted runners do not cross a fork
//! boundary. A fork cannot claim its parent's runners; if it could, forking a
//! repository would be a way to run code on the parent's infrastructure.
//!
//! What makes that worth a permanent check is the shape of the failure. A job
//! that no runner can claim does not error, does not annotate the commit and
//! does not appear as a red X. It sits in `queued` for twenty-four hours and is
//! then canceled. On this fork the consequence was that CI had never once
//! executed — not on a feature branch, not on a pull request, not on a merge to
//! `master` — and the only visible symptom was an Actions tab with nothing in
//! it, which looks exactly like a repository that has not been pushed to yet.
//!
//! A test cannot ask GitHub whether a runner exists. What it can do is assert
//! the property that made the outage possible: that nothing here depends on
//! infrastructure the reader of this repository may not have. A self-hosted
//! label is still allowed — through a variable with a hosted fallback, so the
//! organization that owns runners uses them and everybody else still gets a
//! result.
//!
//! What this does not prove: that the hosted runner it falls back to can
//! actually complete these jobs. `ubuntu-latest` having Docker, enough disk and
//! a working service-container implementation is a fact about GitHub's images,
//! and the only thing that establishes it is a green run.

use std::path::PathBuf;

use css_checks::repo_root;

/// Label prefixes for GitHub-hosted runner images.
///
/// The property is obtainability, not this exact list: anyone who can read this
/// repository can run a job on one of these, with no runner to register, no
/// organization to belong to and nothing to configure.
const HOSTED: [&str; 3] = ["ubuntu-", "windows-", "macos-"];

/// The matrix key this check knows how to follow. See `resolves` for why a
/// different one is a failure rather than something to skip over.
const MATRIX_KEY: &str = "os";

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
    assert!(
        !out.is_empty(),
        "no workflow files under {} -- this check reads them by extension, so a \
         rename or a move turns it into a test that passes by finding nothing",
        dir.display()
    );
    out
}

fn is_hosted(label: &str) -> bool {
    HOSTED.iter().any(|p| label.starts_with(p))
}

/// Strip surrounding quotes and whitespace from a scalar YAML value.
fn scalar(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

/// Can a clone with no configuration obtain the runner this value names?
fn resolves(value: &str) -> Result<(), String> {
    let v = scalar(value);

    if is_hosted(v) {
        return Ok(());
    }

    // `${{ matrix.os }}` defers the answer to the matrix. That is safe rather
    // than a hole in this check only because the matrix's own `os:` entries are
    // collected and checked by the same pass -- so a matrix leg naming a
    // self-hosted label is caught where it is written.
    if let Some(rest) = v.strip_prefix("${{").and_then(|r| r.strip_suffix("}}")) {
        let expr = rest.trim();

        if let Some(key) = expr.strip_prefix("matrix.") {
            return if key.trim() == MATRIX_KEY {
                Ok(())
            } else {
                // Not skipped. This check follows exactly one matrix key, and a
                // job that uses another one would be waved through unexamined
                // -- which is the failure this file exists to prevent, wearing a
                // different name.
                Err(format!(
                    "defers to matrix key `{key}`, but this check only follows \
                     `matrix.{MATRIX_KEY}`. Teach it the new key rather than \
                     leaving the leg unchecked."
                ))
            };
        }

        // An expression resolves for everybody exactly when its fallback is
        // hosted. An undefined repository variable is the empty string, which is
        // falsy in a GitHub expression, so the right-hand operand of `||` is
        // what a clone with nothing configured actually runs on.
        if let Some((_, fallback)) = expr.rsplit_once("||") {
            let f = scalar(fallback);
            return if is_hosted(f) {
                Ok(())
            } else {
                Err(format!(
                    "falls back to `{f}`, which is not a GitHub-hosted runner. \
                     A clone with no `vars` configured would queue forever."
                ))
            };
        }

        return Err(format!(
            "is an expression with no `||` fallback, so what it resolves to \
             depends on configuration this repository does not carry: `{expr}`"
        ));
    }

    Err(format!(
        "names `{v}`, which is neither a GitHub-hosted runner nor an expression \
         with a hosted fallback. If it is a self-hosted label, write it as \
         `${{{{ vars.CI_RUNNER || 'ubuntu-latest' }}}}` and set the variable, so \
         a fork or a clone still gets a result."
    ))
}

/// Every `runs-on:` and every matrix `os:` in the file, with line numbers.
fn runner_values(text: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        for key in ["runs-on:", "- os:", "os:"] {
            if let Some(value) = t.strip_prefix(key) {
                let v = value.trim();
                if !v.is_empty() {
                    out.push((
                        i + 1,
                        key.trim_start_matches("- ").to_string(),
                        v.to_string(),
                    ));
                }
                break;
            }
        }
    }
    out
}

#[test]
fn every_job_names_a_runner_a_clone_can_obtain() {
    let mut failures = Vec::new();

    for path in workflow_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            failures.push(format!("{} could not be read", path.display()));
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let values = runner_values(&text);

        // A workflow with jobs always has a `runs-on:`. Finding none means this
        // check stopped reading the file correctly -- an indentation change, a
        // flow-style rewrite -- and a check that finds nothing passes silently,
        // which is the same class of invisible as the bug it guards.
        assert!(
            values.iter().any(|(_, k, _)| k == "runs-on:"),
            "{name} yielded no `runs-on:` lines. Either it has no jobs, or this \
             check can no longer parse it -- and the second is indistinguishable \
             from a pass."
        );

        for (line, key, value) in values {
            if let Err(why) = resolves(&value) {
                failures.push(format!("{name}:{line}  `{key} {value}` {why}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "a job here can only run on infrastructure a clone of this repository \
         may not have. A job no runner can claim does not fail -- it queues, \
         then is canceled after twenty-four hours, reporting nothing.\n\n{}",
        failures.join("\n")
    );
}
