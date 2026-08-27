//! `run_node` has two branches, and they must hand the driver the same world.
//!
//! `e2e/stack.sh`'s `run_node` runs a driver either as a host process
//! (`--provision=external`, which is CI) or inside the pinned node image
//! (podman or docker, which is reaper and the workstation). Each branch builds
//! the driver's environment separately -- one as a command prefix, the other as
//! a list of `-e` flags -- and nothing made them agree.
//!
//! They did not agree. The container branch passed `CSS_DB_ENCODING` and the
//! host branch did not, so under `--provision=external` the drivers read their
//! own default of `UTF8` and believed a LATIN1 cluster could store anything.
//! Both drivers handle a non-UTF-8 database correctly -- `fuzz.mjs` drops the
//! astral-plane corpus entries because they reproduce a known 500 on every
//! route that writes text, and `concurrency.mjs` records a pinned finding and
//! skips the invite race for the same reason. Neither ran. CI fired 🐴 and 🥧 at
//! a LATIN1 cluster, got SQLSTATE 22P05 both times, and reported a known and
//! already-documented defect as a fresh fuzz finding.
//!
//! The cost of that is not the red build, it is the credibility of the tier:
//! every genuinely new finding is now behind a wall of noise that reproduces on
//! every route that writes text.
//!
//! This is the same shape as the two `From` implementations in
//! `server/src/api/errors.rs` that mapped one unique-violation to two different
//! statuses: two paths to one outcome, and only one of them told the truth. The
//! assertion is the same too -- make the two agree with *each other* rather than
//! with a third list this file would have to keep in step.
//!
//! What this does not prove: that either branch passes the *right* values. It
//! proves only that neither branch silently knows something the other does not.

use std::collections::BTreeSet;

use css_checks::repo_root;

/// `NAME=` written as a command prefix, on a continued line.
///
/// The trailing backslash is what distinguishes an environment prefix from an
/// ordinary assignment: `PATH="$(dirname ...)"` inside the same branch is a
/// local variable, not something handed to the driver, and it does not continue
/// a command.
fn prefix_assignments(branch: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in branch.lines() {
        let t = line.trim();
        if !t.ends_with('\\') {
            continue;
        }
        // Every assignment on the line, not just the first. `sql_ro` writes
        // `PGPASSWORD=... PGOPTIONS=... \` on one line, and a parser that took
        // only the leading token would report PGOPTIONS as missing from the host
        // branch -- which is how this check nearly reported the suite's
        // read-only database guarantee as absent in CI when it is present.
        for token in t.trim_end_matches('\\').split_whitespace() {
            let Some((name, _)) = token.split_once('=') else {
                // The first non-assignment token is the command; nothing after
                // it is part of the environment.
                break;
            };
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            {
                out.insert(name.to_string());
            } else {
                break;
            }
        }
    }
    out
}

/// `-e NAME=` flags passed to the engine.
fn engine_env_flags(branch: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in branch.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("-e ") else {
            continue;
        };
        if let Some((name, _)) = rest.split_once('=') {
            out.insert(name.trim().to_string());
        }
    }
    out
}

/// Every function in `e2e/stack.sh` that branches on the provisioning mode.
///
/// Derived from the file rather than listed here, so a fourth one cannot be
/// added without this check noticing it.
fn dual_path_functions(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    let mut body: Vec<&str> = Vec::new();

    for line in text.lines() {
        if let Some(name) = line.strip_suffix("() {") {
            if !name.contains(char::is_whitespace) {
                current = Some(name.to_string());
                body.clear();
                continue;
            }
        }
        if let Some(name) = current.clone() {
            if line == "}" {
                let joined = body.join("\n");
                if joined.contains("PROVISION} == \"external\"") {
                    out.push((name, joined));
                }
                current = None;
            } else {
                body.push(line);
            }
        }
    }
    out
}

/// Split a dual-path body into (host branch, engine branch).
///
/// Two shapes appear: `if external ... else ... fi`, and `if external ...
/// return ... fi` with the engine path following. Both are handled, because a
/// check that silently skipped the second shape would have missed `start_edge`.
fn branches(body: &str) -> Option<(String, String)> {
    let start = body.find("== \"external\" ]]; then")?;
    let rest = &body[start..];
    for sep in ["\n  else\n", "\n  fi\n"] {
        if let Some(at) = rest.find(sep) {
            return Some((rest[..at].to_string(), rest[at + sep.len()..].to_string()));
        }
    }
    None
}

#[test]
fn both_provisioning_paths_pass_the_same_environment() {
    let stack = repo_root().join("e2e/stack.sh");
    let text = std::fs::read_to_string(&stack)
        .unwrap_or_else(|_| panic!("cannot read {}", stack.display()));

    let functions = dual_path_functions(&text);
    assert!(
        functions.len() >= 5,
        "found only {} functions branching on PROVISION in e2e/stack.sh, which is \
         too few to be right -- this check reads them by shape, so a reformat \
         turns it into a permanent silent pass",
        functions.len()
    );

    let mut compared = Vec::new();
    let mut failures = Vec::new();

    for (name, body) in &functions {
        let Some((host, engine)) = branches(body) else {
            // A guard clause -- `[[ external ]] && return 0` -- has one path, not
            // two, so there is nothing to compare. Skipped only when the function
            // hands a process no environment at all: if it passes one and this
            // check cannot find both sides of it, that is the unchecked function
            // this file exists to prevent, and it fails.
            if prefix_assignments(body).is_empty() && engine_env_flags(body).is_empty() {
                continue;
            }
            failures.push(format!(
                "{name}: passes an environment but branches on PROVISION in a shape \
                 neither `else` nor `fi` splits, so this check cannot compare the \
                 two sides. Teach it the new shape rather than leaving the function \
                 unchecked."
            ));
            continue;
        };

        let host_env = prefix_assignments(&host);
        let engine_env = engine_env_flags(&engine);
        if host_env.is_empty() && engine_env.is_empty() {
            continue;
        }
        compared.push(name.clone());

        let host_only: Vec<&String> = host_env.difference(&engine_env).collect();
        let engine_only: Vec<&String> = engine_env.difference(&host_env).collect();
        if !host_only.is_empty() || !engine_only.is_empty() {
            failures.push(format!(
                "{name}: the two paths hand the process different environments.\n    \
                 only on the host (--provision=external, i.e. CI): {host_only:?}\n    \
                 only in the container (podman/docker, i.e. reaper): {engine_only:?}"
            ));
        }
    }

    // The three that pass an environment at all. Named so that a refactor which
    // stops this check seeing one of them fails here rather than quietly
    // reducing what is compared.
    for required in ["run_node", "start_server", "start_edge"] {
        assert!(
            compared.iter().any(|n| n == required),
            "{required} passes an environment to a process and was not compared; \
             examined: {compared:?}"
        );
    }

    assert!(
        failures.is_empty(),
        "a process behaves differently depending on how the stack was \
         provisioned, and the difference is invisible until the two branches are \
         compared side by side.\n\n{}",
        failures.join("\n")
    );
}

/// Does `haystack` mention `name` as a whole identifier?
fn mentions(haystack: &str, name: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(name) {
        let start = from + at;
        let end = start + name.len();
        let before_ok = start == 0 || !ident(bytes[start - 1] as char);
        let after_ok = end >= bytes.len() || !ident(bytes[end] as char);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Every variable a driver reads must be something `run_node` can convey.
///
/// The sibling test above asserts the two branches agree with each other. They
/// did agree, and were both wrong: neither conveyed `CSS_FUZZ_ITERATIONS`,
/// `CSS_FUZZ_SEED`, `CSS_FUZZ_BATCH`, `CSS_RACE_ROUNDS`, `CSS_RACE_FANOUT` or
/// `CSS_RUN_TAG`. That limitation is written into that test's own doc comment,
/// and it went on to cost exactly what it warned about.
///
/// The asymmetry is easy to miss and impossible to see from either side alone.
/// The host branch runs node in the same shell, so it inherits anything
/// exported into the run and needs no help -- which is why `--provision=external`
/// honoured every one of these and CI ran the fuzzer at its configured 600. A
/// container inherits nothing, so under podman all six silently took their
/// defaults: the fuzzer ran 400 iterations whatever the profile said, the
/// concurrency tier used default rounds and fanout, and `CSS_FUZZ_SEED` did
/// nothing whatsoever -- while `SUMMARY.md` printed a replay command built
/// around that seed on every single run.
///
/// A documented procedure that cannot work is worse than a missing one. Somebody
/// reproducing a finding would have got a different path and concluded the
/// finding was flaky.
#[test]
fn run_node_can_convey_everything_the_drivers_read() {
    let root = repo_root();
    let stack = std::fs::read_to_string(root.join("e2e/stack.sh"))
        .expect("e2e/stack.sh must be readable from the repo root");

    let body = {
        let start = stack
            .find("run_node() {")
            .expect("e2e/stack.sh must define run_node()");
        let rest = &stack[start..];
        let end = rest.find("\n}\n").map(|e| e + start).unwrap_or(stack.len());
        stack[start..end].to_string()
    };

    // What the drivers ask for.
    let mut wanted: Vec<String> = Vec::new();
    let drivers = root.join("e2e/drivers");
    let entries = std::fs::read_dir(&drivers).expect("e2e/drivers must exist");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "mjs") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut from = 0;
        while let Some(at) = src[from..].find("process.env.") {
            let start = from + at + "process.env.".len();
            let name: String = src[start..]
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if !name.is_empty() && !wanted.contains(&name) {
                wanted.push(name);
            }
            from = start;
        }
    }
    wanted.sort();

    assert!(
        wanted.len() >= 6,
        "found only {} `process.env.*` reads across e2e/drivers, which is too few \
         to be right -- this check reads them by text, so a refactor turns it into \
         a permanent silent pass: {wanted:?}",
        wanted.len()
    );

    // Comments stripped first. The block that forwards these variables is
    // introduced by a comment naming several of them, and matching against that
    // prose reported them as conveyed when the code conveying them had been
    // deleted -- so the check would have passed on the exact bug it exists for,
    // reporting four of six. The mutation check caught it; reading the code did
    // not.
    let code: String = body
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    let missing: Vec<&String> = wanted.iter().filter(|n| !mentions(&code, n)).collect();

    assert!(
        missing.is_empty(),
        "a driver reads configuration that `run_node` never conveys, so under \
         --provision=podman it silently takes its default and the run reports \
         success at the wrong settings.\n\n  {missing:?}\n\nThe host branch \
         inherits the environment and hides this; only the container path is \
         affected, which is the path the workstation and every reaper session \
         use."
    );
}
