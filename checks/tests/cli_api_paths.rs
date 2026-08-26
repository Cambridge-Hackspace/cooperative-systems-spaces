//! Every server path the CLI asks for must be reachable.
//!
//! This exists because one was not. `cli/src/auth.rs` posted to
//! `/auth/register` while every other call in the crate used `/api/...`, so
//! `css-cli auth register` never reached the API at all — it fell through to
//! the server's static-file fallback and returned whatever that serves. No test
//! could see it, because the CLI's request layer is entirely async and
//! network-bound, and nothing compared the two conventions.
//!
//! What this asserts is the convention, not a route table: the CLI has exactly
//! one prefix for API calls, and every path it builds uses it. The full
//! route-existence check against `server/src/api/` lands with the rest of the
//! source-as-data tier; this is the half that can be stated without it, and it
//! is the half that would have caught the defect.

use std::collections::BTreeSet;
use walkdir::WalkDir;

/// Paths the CLI passes to `ApiClient::{get,post,put,delete,request_raw}`.
///
/// Extracted as *every string literal beginning with a slash* in the crate,
/// rather than by matching call arguments. `cli/src/commands/users.rs` and the
/// server client both build paths with `format!`, so an argument-matching
/// scraper would quietly see fewer of them over time — and a scraper that finds
/// less than it used to is indistinguishable from a codebase that got smaller.
fn cli_paths() -> BTreeSet<String> {
    let root = css_checks::repo_root();
    let mut found = BTreeSet::new();

    for entry in WalkDir::new(root.join("cli/src"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
    {
        let src = std::fs::read_to_string(entry.path()).expect("walkdir yielded a readable file");
        for line in src.lines() {
            // Skip our own documentation of the defect, and comments generally:
            // a check that reads prose as code will eventually be satisfied by
            // prose, which is the "copy can satisfy a check by quoting itself"
            // failure the methodology names.
            let code = line.split("//").next().unwrap_or("");
            for literal in string_literals(code) {
                if literal.starts_with('/') {
                    found.insert(literal);
                }
            }
        }
    }

    assert!(
        !found.is_empty(),
        "found no slash-leading string literals under cli/src — the scraper is \
         broken, and an empty set would make every assertion below vacuous"
    );
    found
}

/// Double-quoted literals on one line. Deliberately simple: it does not handle
/// raw strings or escaped quotes, and it does not need to — anything it cannot
/// parse it skips, and the emptiness assertion above catches a scraper that
/// stopped working altogether.
fn string_literals(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut lit = String::new();
        for c in chars.by_ref() {
            if c == '"' {
                break;
            }
            lit.push(c);
        }
        out.push(lit);
    }
    out
}

#[test]
fn every_cli_api_path_carries_the_api_prefix() {
    // Paths the CLI uses that are deliberately not under /api. Each entry is a
    // route this check stops watching, so each carries its reason.
    const NOT_API: &[&str] = &[
        // The JSON status handler is merged at the router root, not nested
        // under /api — see `general_route` in server/src/main.rs:307-308.
        "/status",
    ];

    let offenders: Vec<_> = cli_paths()
        .into_iter()
        .filter(|p| !p.starts_with("/api/"))
        .filter(|p| !NOT_API.contains(&p.as_str()))
        .collect();

    assert!(
        offenders.is_empty(),
        "these CLI paths do not start with /api/, so they miss the server's API \
         router entirely and fall through to its static-file fallback: {offenders:?}"
    );
}

#[test]
fn the_cli_actually_calls_the_endpoints_this_check_is_about() {
    // Guards the guard. If the scraper ever stops finding the auth endpoints —
    // a rename, a refactor into a builder, a move to another crate — the test
    // above would keep passing over a shrinking set and report a convention
    // nobody follows any more.
    let paths = cli_paths();
    for expected in ["/api/auth/login", "/api/auth/register", "/api/auth/me"] {
        assert!(
            paths.contains(expected),
            "expected the CLI to call {expected}; found {paths:?}"
        );
    }
}
