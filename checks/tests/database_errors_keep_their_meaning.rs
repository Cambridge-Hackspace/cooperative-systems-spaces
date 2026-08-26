//! A ratchet on handlers that throw away what a database error knew.
//!
//! `ApiError::from(DatabaseError)` classifies: `NotFound` becomes 404, a unique
//! violation becomes 409, a foreign-key violation becomes 409, a not-null or
//! check violation becomes 400, and everything genuinely ours becomes 500.
//! Handlers that write
//!
//! ```ignore
//! state.db.update_tool(id, &payload).map_err(|e| {
//!     tracing::error!("Failed to update tool: {}", e);
//!     ApiError::InternalServerError("Failed to update tool".to_string())
//! })?
//! ```
//!
//! discard all of it. Updating a tool that does not exist answered 500 instead
//! of 404; a colliding name answered 500 instead of 409. The seeded fuzz tier
//! found that one by asking for an id that matches nothing — the first thing
//! anybody would try by hand, and the last thing anybody writes a test for.
//!
//! **This is a ratchet, not a gate.** There are dozens of these and converting
//! them all in one change would be a large diff touching every handler in the
//! API, reviewed by nobody, in exchange for status codes nothing yet asserts.
//! So the count is pinned and may only go down. Fixing one is a normal-sized
//! piece of work; adding one fails here.
//!
//! The same shape as the TypeScript strictness ratchet, and for the same
//! reason: unify the direction, not the date.

use css_checks::repo_root;

/// The current number of sites, per file.
///
/// Written out per file rather than as one total, so that a fix in one handler
/// and a regression in another cannot cancel out — which is the failure mode
/// of every ratchet expressed as a single number.
const BUDGET: &[(&str, usize)] = &[
    ("admin.rs", 11),
    ("auth.rs", 3),
    ("calendar.rs", 0),
    ("config.rs", 0),
    ("devices.rs", 0),
    ("doors.rs", 0),
    ("errors.rs", 10),
    ("home_links.rs", 0),
    ("instance.rs", 0),
    ("mfa.rs", 8),
    ("mod.rs", 0),
    ("pages.rs", 0),
    ("places.rs", 0),
    ("profiles.rs", 5),
    ("responses.rs", 0),
    ("schedules.rs", 3),
    ("toolguard.rs", 17),
    ("tools.rs", 13),
    ("trainers.rs", 12),
    ("training.rs", 27),
    ("users.rs", 2),
    ("webhooks.rs", 0),
];

fn counts() -> Vec<(String, usize)> {
    let api = repo_root().join("server/src/api");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&api).expect("server/src/api must exist") {
        let path = entry.expect("readable").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let n = src
            .lines()
            .filter(|l| {
                let code = l.split("//").next().unwrap_or("");
                code.contains("ApiError::InternalServerError")
            })
            .count();
        out.push((name, n));
    }
    out.sort();
    out
}

#[test]
fn the_scan_found_the_api_module() {
    let counts = counts();
    assert!(
        counts.len() >= 15,
        "found only {} files under server/src/api; the scan is broken",
        counts.len()
    );
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    assert!(
        total >= 50,
        "counted only {total} InternalServerError sites; the scan is broken and \
         the ratchet below would pass over nothing"
    );
}

#[test]
fn no_file_gained_a_blanket_500() {
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    let mut unlisted = Vec::new();

    for (file, actual) in counts() {
        match BUDGET.iter().find(|(f, _)| *f == file) {
            Some((_, budget)) => {
                if actual > *budget {
                    regressions.push(format!("{file}: {budget} -> {actual}"));
                } else if actual < *budget {
                    improvements.push(format!("{file}: {budget} -> {actual}"));
                }
            }
            None => unlisted.push(format!("{file}: {actual}")),
        }
    }

    assert!(
        regressions.is_empty(),
        "these files gained `ApiError::InternalServerError` sites:\n{}\n\n\
         A handler that maps a DatabaseError to a bare 500 discards what the \
         error knew: a missing row becomes 500 instead of 404, a collision \
         becomes 500 instead of 409, and the caller is told the server broke \
         about something they can fix. Log it and return `ApiError::from(e)`.\n\n\
         If the error genuinely is ours -- a serialisation failure, a config \
         problem -- 500 is right, and the budget in this file goes up with the \
         reason in the commit message.",
        regressions.join("\n")
    );

    assert!(
        unlisted.is_empty(),
        "new files under server/src/api are not in the ratchet's \
         budget:\n{}\n\nAdd them with their current count.",
        unlisted.join("\n")
    );

    // The ratchet has to tighten when somebody does the work, or it stops
    // meaning anything within a month.
    assert!(
        improvements.is_empty(),
        "these files now have FEWER blanket 500s than the budget allows -- good \
         news, and the budget has to come down to match or the ground that was \
         won is given back:\n{}\n\n\
         Update BUDGET in this file.",
        improvements.join("\n")
    );
}

#[test]
fn the_one_the_fuzz_tier_found_is_actually_fixed() {
    // A ratchet says "no worse". This says the specific defect is gone, which a
    // count cannot: the number would be identical if somebody deleted a
    // different InternalServerError and reinstated this one.
    let src = std::fs::read_to_string(repo_root().join("server/src/api/tools.rs"))
        .expect("server/src/api/tools.rs must exist");
    let at = src
        .find("async fn update_tool(")
        .expect("update_tool must still exist");
    let body = &src[at..];
    let end = body.find("\n}\n").unwrap_or(body.len());
    let body = &body[..end];

    assert!(
        body.contains("ApiError::from(e)"),
        "update_tool no longer converts the database error. Updating a tool that \
         does not exist answers 500 again, rather than 404."
    );
    assert!(
        !body.contains("ApiError::InternalServerError"),
        "update_tool is back to a blanket 500"
    );
}
