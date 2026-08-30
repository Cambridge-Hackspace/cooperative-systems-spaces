//! An environment variable set in `.reaper.toml` must be read by something.
//!
//! `[profiles.nightly]` set `FUZZ_ITERATIONS = "4000"`. The fuzz driver reads
//! `CSS_FUZZ_ITERATIONS`. So the profile whose entire purpose is a long hunt ran
//! the fuzzer at its default of 400 -- an eighth of what it was configured for
//! -- and reported success either way.
//!
//! That is the shape worth guarding: a misconfiguration that cannot fail. There
//! is no error, no warning, and no observable difference except that the tier
//! did less work than the file says. It went unnoticed while the same mistake
//! was found and fixed in the CI workflow, because a workflow that runs is read
//! and a profile that is invoked occasionally is not.
//!
//! The cost was concrete. The fuzz tier is a seeded search over 165 routes, and
//! at 400 iterations a run samples a small part of that space. Two consecutive
//! CI runs each found a different route answering 500 where a 4xx was owed --
//! the class TESTING.md section 9 tracks as 109 remaining sites. A nightly at
//! its intended 4000 would have been finding those in batches, months earlier
//! and away from anybody's push.
//!
//! What this does not prove: that the value is sensible, or that the variable
//! reaches the process that wants it. `run_node`'s two branches disagreeing
//! about `CSS_DB_ENCODING` is that failure, and
//! `both_driver_paths_pass_the_same_env.rs` is what catches it.

use std::collections::BTreeSet;

use css_checks::repo_root;

/// Set in `.reaper.toml` and deliberately read by nothing yet.
///
/// Each entry needs a reason that covers exactly this variable. "It will be
/// used later" is a reason; a bare name in this list is how the check gets
/// hollowed out one exemption at a time.
const NOT_YET_READ: [(&str, &str); 2] = [
    (
        "JOURNEY_ITERATIONS",
        "the Tier 9 journey driver is not written -- the oracle exists and the \
         driver that would accumulate a world for it does not. See TESTING.md \
         section 7. Configuration for a tier that does not exist yet, not a typo.",
    ),
    (
        "JOURNEY_SEEDS",
        "same driver, same reason: [profiles.hunt] configures a tier that has no \
         driver.",
    ),
];

/// Names assigned inside `env = { ... }` tables in `.reaper.toml`.
fn configured_env(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        let Some(rest) = t.strip_prefix("env = {") else {
            continue;
        };
        let rest = rest.trim_end_matches('}');
        for pair in rest.split(',') {
            if let Some((name, _)) = pair.split_once('=') {
                let name = name.trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

/// Does `corpus` mention `name` as a whole identifier?
///
/// Not `contains`. `FUZZ_ITERATIONS` is a substring of `CSS_FUZZ_ITERATIONS`,
/// so a plain containment test reports the unprefixed name as read whenever the
/// prefixed one appears -- which is precisely the bug this file was written for,
/// and precisely the case it would then have waved through. The mutation check
/// caught that; nothing else would have.
fn mentions_identifier(corpus: &str, name: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let bytes = corpus.as_bytes();
    let mut from = 0;
    while let Some(at) = corpus[from..].find(name) {
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

/// Every file under `e2e/` that could read an environment variable.
fn e2e_sources() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![repo_root().join("e2e")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e == "sh" || e == "mjs" || e == "js")
            {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn every_variable_a_profile_sets_is_read_by_something() {
    let manifest = repo_root().join(".reaper.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|_| panic!("cannot read {}", manifest.display()));

    let configured = configured_env(&text);
    assert!(
        !configured.is_empty(),
        "no `env = {{ ... }}` tables found in .reaper.toml. This check reads them \
         by line, so a reformat turns it into a permanent silent pass."
    );

    // The suite's own files, plus the manifest's `cmd` strings -- and only
    // those. `CSS_E2E_STAGES` is consumed by
    // `--only ${CSS_E2E_STAGES:-default}` in [run] cmd rather than by anything
    // under e2e/, so it is read; but taking the whole manifest as corpus would
    // make every configured variable trivially find itself and this check would
    // assert nothing at all.
    let mut corpus: String = e2e_sources()
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("cmd = ") {
            corpus.push('\n');
            corpus.push_str(t);
        }
    }
    assert!(
        corpus.len() > 10_000,
        "read only {} bytes from e2e/, which is too little to be the suite -- \
         this check would then report every variable as unread",
        corpus.len()
    );

    let mut unread = Vec::new();
    for name in &configured {
        if mentions_identifier(&corpus, name.as_str()) {
            continue;
        }
        match NOT_YET_READ.iter().find(|(n, _)| n == name) {
            Some(_) => {}
            None => unread.push(name.clone()),
        }
    }

    // An exemption that stops being true is a lie the file keeps telling.
    let stale: Vec<&str> = NOT_YET_READ
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| mentions_identifier(&corpus, n))
        .collect();
    assert!(
        stale.is_empty(),
        "these are listed as not-yet-read and something now reads them: {stale:?}. \
         Delete the entry -- an exemption nobody removes is how the next real one \
         gets waved through."
    );

    assert!(
        unread.is_empty(),
        ".reaper.toml sets environment variables that nothing under e2e/ reads, so \
         a profile configures a value the run never sees and reports success \
         anyway.\n\n  {unread:?}\n\nIf a name is deliberately ahead of its \
         driver, add it to NOT_YET_READ with a reason covering exactly that name."
    );
}
