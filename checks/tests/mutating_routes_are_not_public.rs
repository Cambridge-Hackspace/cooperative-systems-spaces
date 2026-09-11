//! Tier 3: no state-mutating route is `Guard::Public` unless it is on a short,
//! justified allowlist.
//!
//! `route_table_matches` proves the hand-written route table in
//! `server/tests/common/mod.rs` equals the router, and `contract_matrix` proves
//! every *guarded* route rejects a request with no valid credential. Neither
//! stops a NEW non-GET route from being classified `Public` by accident -- a
//! mutating endpoint reachable with no authentication at all. A human writes the
//! guard column, so nothing independent notices when a POST lands there.
//!
//! This reads the same table (safe: `route_table_matches` keeps it equal to the
//! router) and asserts every POST/PUT/PATCH/DELETE marked `Public` is one that
//! was deliberately chosen to be, each with a reason. A GET is exempt: a
//! read-only endpoint being public is a disclosure decision the contract tier
//! already reasons about, not a missing-auth-on-a-write bug.

use css_checks::read;
use std::collections::BTreeSet;

/// The mutating verbs. A `Public` one of these is what this check polices.
const MUTATING: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];

/// Routes that are `Public` on purpose, each with the reason it needs no guard.
/// Adding a mutating public route means adding it here with a justification --
/// which is the point: the decision becomes explicit and reviewed.
const ALLOWED_PUBLIC: &[(&str, &str, &str)] = &[
    // Pre-authentication auth flows: the caller has no credential yet, or is
    // exchanging one kind for another. Each authenticates from its own body.
    (
        "POST",
        "/api/auth/login",
        "issues the session; no credential yet",
    ),
    (
        "POST",
        "/api/auth/logout",
        "clears the session cookie; safe without a valid token",
    ),
    (
        "POST",
        "/api/auth/register",
        "creates the account; no credential yet",
    ),
    (
        "POST",
        "/api/auth/email/verify",
        "consumes an emailed single-use token carried in the body",
    ),
    (
        "POST",
        "/api/auth/email/resend",
        "re-sends verification; gated by its own throttle, not a session",
    ),
    (
        "POST",
        "/api/auth/mfa/verify",
        "second factor of login; the primary factor is proven in the body",
    ),
    (
        "POST",
        "/api/auth/password-reset/request",
        "starts reset for an address; deliberately answers the same for any input",
    ),
    (
        "POST",
        "/api/auth/password-reset/consume",
        "consumes an emailed single-use reset token carried in the body",
    ),
    // Token / invite / signature exchanges: the credential is in the request,
    // checked inside the handler rather than by a route guard.
    (
        "POST",
        "/api/cmi5/fetch",
        "exchanges a single-use launch token (in the body) for LRS credentials",
    ),
    (
        "POST",
        "/api/devices/register",
        "device onboarding via a single-use invite code checked in the handler",
    ),
    (
        "POST",
        "/api/groupsio/webhook",
        "inbound webhook authenticated by an HMAC signature, not a session",
    ),
    (
        "POST",
        "/api/stripe/webhook",
        "inbound webhook authenticated by the Stripe signature, not a session",
    ),
];

/// `(METHOD, path, guard)` for every `R(...)` row in the contract route table.
fn table_rows() -> Vec<(String, String, String)> {
    let src = read("server/tests/common/mod.rs");
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with("R(") {
            continue;
        }
        // The two quoted fields are METHOD and path.
        let quoted: Vec<&str> = line
            .match_indices('"')
            .collect::<Vec<_>>()
            .chunks(2)
            .filter_map(|w| {
                let (a, _) = w.first()?;
                let (b, _) = w.get(1)?;
                Some(&line[a + 1..*b])
            })
            .collect();
        let guard = line
            .split("Guard::")
            .nth(1)
            .and_then(|g| g.split(|c: char| !c.is_alphanumeric()).next())
            .unwrap_or("");
        if quoted.len() == 2 && !guard.is_empty() {
            out.push((
                quoted[0].to_string(),
                quoted[1].to_string(),
                guard.to_string(),
            ));
        }
    }
    out
}

#[test]
fn the_route_table_was_actually_read() {
    // Guards the guard: an empty or unparsed table would make the check below
    // pass over nothing. The contract tier itself asserts the table is large;
    // here we only need to know the parser saw a representative slice, including
    // both a mutating public route (so the ALLOWED set is exercised) and a
    // guarded one (so "Public" is a real discriminator, not the only value seen).
    let rows = table_rows();
    assert!(
        rows.len() > 150,
        "parsed only {} rows from the route table; the parser is broken",
        rows.len()
    );
    assert!(
        rows.iter()
            .any(|(m, _, g)| MUTATING.contains(&m.as_str()) && g == "Public"),
        "no mutating Public route parsed at all -- the scan or the table changed \
         shape, and this check would pass vacuously"
    );
    assert!(
        rows.iter().any(|(_, _, g)| g == "Admin"),
        "no Admin route parsed -- guard extraction is broken, so `Public` below \
         is not being distinguished from anything"
    );
}

#[test]
fn no_mutating_route_is_public_without_a_reason() {
    let allowed: BTreeSet<(&str, &str)> = ALLOWED_PUBLIC.iter().map(|(m, p, _)| (*m, *p)).collect();

    let offenders: Vec<String> = table_rows()
        .into_iter()
        .filter(|(m, _, g)| MUTATING.contains(&m.as_str()) && g == "Public")
        .filter(|(m, p, _)| !allowed.contains(&(m.as_str(), p.as_str())))
        .map(|(m, p, _)| format!("{m} {p}"))
        .collect();

    assert!(
        offenders.is_empty(),
        "these state-mutating routes are `Guard::Public` -- reachable with no \
         authentication at all:\n{}\n\n\
         If that is a mistake, give the route a guard (and update the contract \
         route table). If it is deliberate -- a pre-auth flow, or a body/\
         signature-authenticated endpoint -- add it to ALLOWED_PUBLIC in this \
         file with the reason it needs no guard.",
        offenders.join("\n")
    );
}

#[test]
fn the_allowlist_has_no_stale_entries() {
    // Every allowlisted route must still exist in the table as a mutating Public
    // route. An entry that no longer matches (route deleted, or it gained a
    // guard) is dead permission and should be removed, or it hides the next
    // route that reuses that path.
    let rows: BTreeSet<(String, String, String)> = table_rows().into_iter().collect();
    let stale: Vec<String> = ALLOWED_PUBLIC
        .iter()
        .filter(|(m, p, _)| !rows.contains(&(m.to_string(), p.to_string(), "Public".to_string())))
        .map(|(m, p, _)| format!("{m} {p}"))
        .collect();

    assert!(
        stale.is_empty(),
        "these ALLOWED_PUBLIC entries no longer match a mutating Public route in \
         the table (route removed, or it gained a guard) -- remove them:\n{}",
        stale.join("\n")
    );
}
