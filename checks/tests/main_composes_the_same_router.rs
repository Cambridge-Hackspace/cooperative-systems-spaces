//! The contract tier must test the router production serves.
//!
//! `server/tests/contract_matrix.rs` builds its router with
//! `Router::new().nest("/api", api::api_routes())`, because `api_routes()`
//! returns the API surface *without* its prefix and something has to add it.
//! `main.rs` builds the real one. Nothing made those two agree, and the failure
//! mode is the quietest kind there is: the contract tier would keep asserting
//! 998 route/credential pairs against a router nobody serves, reporting
//! complete coverage of a surface that had moved.
//!
//! Three things are checked, all as source data, all reporting drift rather
//! than absorbing it:
//!
//!   1. `main.rs` nests `api::api_routes()` at exactly `/api`;
//!   2. the only routes `main.rs` composes outside that nest are ones this file
//!      lists by name, each with a reason and a statement of which tier does
//!      cover it;
//!   3. no *other* file composes routes at the top level.
//!
//! What this deliberately does not do is move `build_router` into the library
//! so the two could share one implementation. That was the original plan and it
//! is the wrong trade here: the `/api` nest carries `prom.http_layer()`, and
//! dr-metrix lives entirely in the bin because it does not build on every
//! platform this repository is developed on. Threading a generic `Layer`
//! parameter through a library function to satisfy a test would put real type
//! machinery in production code for a test's benefit, and would change which
//! requests the metrics layer sees. Comparing the two statements is cheaper and
//! catches the same drift.

use css_checks::{read, repo_root};

fn main_rs() -> String {
    read("server/src/main.rs")
}

/// Routes `main.rs` is allowed to compose itself, each with the reason it is
/// not in `api_routes()` and the tier that does cover it.
///
/// The list is the check. Adding a route to `main.rs` without adding it here
/// fails, which is the moment to ask whether it belongs in `api_routes()` — and
/// if it genuinely does not, to say here what covers it instead.
const MAIN_ONLY: &[(&str, &str)] = &[
    (
        "/metrics",
        "dr-metrix's exporter. It cannot live in api_routes() because the crate \
         is Linux-only and the library has to build everywhere. Covered by the \
         stack battery's `health` stage, which is the only tier that runs the \
         real binary.",
    ),
    (
        "/status",
        "The liveness endpoint, composed outside /api so that a broken API \
         router still answers it. Covered by the `health` stage, and used as \
         the readiness probe by every other stack stage and as the fuzz tier's \
         still-alive oracle.",
    ),
];

/// Programs under server/src/bin/ that are not the API server and legitimately
/// compose their own routers.
///
/// Each is a separate binary with its own tier. Listing them means the
/// exclusion covers exactly these and not "anything under bin/", which would
/// let a future API surface hide there.
const SEPARATE_BINARIES: &[&str] = &[
    // The standalone webhook sink the stack battery points webhook deliveries
    // at. Covered by the `webhooks` stage, which asserts what it received rather
    // than what the dispatcher claimed to send.
    "bin/webhook_recvr.rs",
    // The standalone SMTP sink the stack battery points [email] at. It composes
    // no HTTP routes at all -- it speaks SMTP over a raw socket -- but it lives
    // under bin/ and this list is what says a binary there is deliberate.
    // Covered by the `mail` stage, which asserts what arrived rather than what
    // the mailer claimed to send.
    "bin/smtp_sink.rs",
    // The standalone Groups.io sink the stack battery points [groupsio] at. It
    // composes the fake Groups.io member API plus a small control surface, none
    // of it the server's own routes. Covered by the `groupsio` stage, which
    // asserts the roster it holds rather than what the sync claimed to do.
    "bin/groupsio_sink.rs",
    // The standalone Stripe sink the stack battery points [stripe] at. It
    // composes a fake of the Stripe API slice the membership module calls plus a
    // small control surface, none of it the server's own routes. Covered by the
    // `stripe` stage, which asserts the ledger balance and role rather than what
    // the client claimed to send.
    "bin/stripe_sink.rs",
];

/// Every `.route("...")` literal in a file, comments stripped.
fn routes_in(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let stripped: String = src
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    for (i, _) in stripped.match_indices(".route(") {
        let rest = &stripped[i..];
        let Some(open) = rest.find('"') else { continue };
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            continue;
        };
        out.push(after[..close].to_string());
    }
    out
}

#[test]
fn main_nests_the_api_router_at_exactly_slash_api() {
    let src = main_rs();
    assert!(
        src.contains(".nest(\"/api\", api::api_routes()"),
        "server/src/main.rs no longer nests api::api_routes() at \"/api\".\n\n\
         The contract tier builds its router as \
         `Router::new().nest(\"/api\", api::api_routes())` and asserts 998 \
         route/credential pairs against it. If production mounts the same \
         routes somewhere else, every one of those assertions is still true and \
         none of them is about the running server any more."
    );
}

#[test]
fn main_composes_nothing_but_the_listed_routes() {
    let found = routes_in(&main_rs());

    // A scraper that found nothing would make the comparison below pass over an
    // empty set, which is exactly the shape this whole file exists to prevent.
    assert!(
        !found.is_empty(),
        "found no .route() calls in server/src/main.rs at all; the scraper is broken"
    );

    let allowed: Vec<&str> = MAIN_ONLY.iter().map(|(path, _)| *path).collect();
    let unexpected: Vec<&String> = found
        .iter()
        .filter(|p| !allowed.contains(&p.as_str()))
        .collect();

    assert!(
        unexpected.is_empty(),
        "server/src/main.rs composes {unexpected:?}, which no tier is known to cover.\n\n\
         Routes in main.rs are invisible to the contract tier, which builds its \
         router from api::api_routes() alone. Either move these into \
         api::api_routes() — where the route table, the parity check and the \
         998-pair matrix all see them — or add them to MAIN_ONLY in this file \
         with the reason they cannot live there and the tier that does cover \
         them."
    );

    // And the reverse: a listed route that is no longer composed. An
    // exemption for something that does not exist is a comment claiming
    // coverage nobody provides.
    for (path, reason) in MAIN_ONLY {
        assert!(
            found.iter().any(|p| p == path),
            "MAIN_ONLY lists {path} — {reason} — but main.rs no longer composes it"
        );
    }
}

#[test]
fn no_other_crate_file_composes_top_level_routes() {
    // `api/**` is where routes belong. Anywhere else in the server crate is a
    // second composition site, and a second composition site is how the first
    // one stops being the whole story.
    let root = repo_root().join("server/src");
    let mut offenders = Vec::new();

    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            // Narrowed to the css-server binary's own tree. server/src/bin/*
            // are separate programs -- webhook_recvr is the standalone sink the
            // webhooks tier points at -- and their routers are not the API
            // surface this check is about. The exclusion covers exactly the
            // binaries named in SEPARATE_BINARIES below and nothing else, so a
            // new one cannot appear here and dodge the check by being new.
            if rel.starts_with("api/") || rel == "main.rs" || rel.starts_with("bin/") {
                if rel.starts_with("bin/") && !SEPARATE_BINARIES.contains(&rel.as_str()) {
                    offenders.push(format!(
                        "server/src/{rel}: a new binary composing its own routes. \
                         If it is a separate program, add it to SEPARATE_BINARIES \
                         with the tier that covers it; if it is part of the API, \
                         its routes belong in api::api_routes()."
                    ));
                }
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            let found = routes_in(&src);
            if !found.is_empty() {
                offenders.push(format!("server/src/{rel}: {found:?}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "routes are composed outside server/src/api/ and server/src/main.rs:\n{}\n\n\
         The contract tier derives its whole picture of the API from \
         api::api_routes(). A route registered anywhere else is served in \
         production and asserted by nothing.",
        offenders.join("\n")
    );
}
