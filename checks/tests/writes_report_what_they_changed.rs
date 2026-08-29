//! Database writers that throw away the number of rows they changed.
//!
//! `diesel::update(...).execute(&mut conn)` and `diesel::delete(...).execute(...)`
//! both answer with a row count. A writer that returns `Result<(), DatabaseError>`
//! and never looks at that count cannot tell "I changed the row you named" from
//! "the row you named does not exist" — and the handler above it then answers
//! 200 for a no-op.
//!
//! This has already been found twice by hand, from opposite ends:
//!
//!   * `remove_tool_trainer` returned 200 for removing a trainer that was not
//!     there, and then wrote an audit entry saying it had. Fixed earlier on
//!     this branch by returning `NotFound` when `affected == 0`.
//!   * `remove_training_prerequisite` is reached with the wrong id entirely —
//!     the frontend sends a `training_steps` id where the server deletes from
//!     `training_prerequisites` — so it matches nothing on every call, answers
//!     200, and the prerequisite stays on screen. Pinned in
//!     `frontend/tests/components/PrerequisitesModal.spec.ts`.
//!
//! Two instances of one shape is a pattern, so this pins the rest of them
//! rather than waiting for the third to be found in production.
//!
//! **A ratchet, not a gate.** Converting eleven writers at once would be a
//! large diff across unrelated features in exchange for status codes that
//! nothing yet asserts, and several of the eleven are genuinely fine. So each
//! one is listed with a verdict, and the list may only shrink: fixing one is
//! normal-sized work, adding one fails here.
//!
//! What this does NOT prove: that any of these are reachable with an id that
//! matches nothing, or that the handler above translates `NotFound` into a 404.
//! It is a source-level claim about what the row count is used for. Tier 6's
//! stack battery owns the round trip.

use css_checks::read;

/// A writer that executes a statement and returns `Result<(), DatabaseError>`.
struct Writer {
    name: &'static str,
    /// Why it is acceptable for this one to ignore the row count, or `None`
    /// when it is not and this is a recorded defect.
    exempt: Option<&'static str>,
}

/// Every `Result<(), DatabaseError>` writer in `database.rs` that runs
/// `.execute(&mut conn)` without reading the result.
///
/// Written out here rather than derived from the file, so that this list is a
/// claim about the code and not a restatement of it. The scan below asserts
/// the two agree in both directions.
const WRITERS: &[Writer] = &[
    Writer {
        name: "health_check",
        exempt: Some(
            "`SELECT 1`. There is no row being addressed, so there is no \
                      count to interpret; the error is the whole answer.",
        ),
    },
    Writer {
        name: "touch_user_webauthn_last_used",
        exempt: Some(
            "A best-effort timestamp on a credential the caller just \
                      used successfully. Nothing branches on it and a miss \
                      costs a stale `last_used_at`, not a wrong decision.",
        ),
    },
    // ---- The rest are recorded defects, worst first. ----
    Writer {
        name: "mark_recovery_code_used",
        exempt: None, // A single-use credential. Matching zero rows means the
                      // code was NOT consumed, and the caller proceeds as
                      // though it was.
    },
    Writer {
        name: "confirm_user_totp",
        exempt: None, // Same shape: enrolment is reported confirmed whether or
                      // not a row moved.
    },
    Writer {
        name: "set_user_mfa_enrolled",
        exempt: None,
    },
    Writer {
        name: "remove_training_prerequisite",
        exempt: None, // Called with the wrong id on every invocation today.
    },
    Writer {
        name: "revoke_instructor_certification",
        exempt: None,
    },
    Writer {
        name: "update_training_step_position",
        exempt: None,
    },
    Writer {
        name: "delete_training_step",
        exempt: None,
    },
    Writer {
        name: "delete_tool",
        exempt: None,
    },
    Writer {
        name: "delete_user",
        exempt: None,
    },
];

/// The number of writers that ignore a row count they should be reading.
///
/// May only go down. If it goes up, a new writer was added in the shape that
/// has already produced two production defects.
const RECORDED_DEFECTS: usize = 9;

/// Extract `pub fn <name>` bodies from `database.rs`, brace-matched.
fn functions(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < bytes.len() {
        let line = bytes[i];
        let Some(rest) = line.trim_start().strip_prefix("pub fn ") else {
            i += 1;
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();

        let mut depth = 0usize;
        let mut seen_open = false;
        let mut body = String::new();
        let mut j = i;
        while j < bytes.len() {
            body.push_str(bytes[j]);
            body.push('\n');
            for c in bytes[j].chars() {
                match c {
                    '{' => {
                        depth += 1;
                        seen_open = true;
                    }
                    '}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            if seen_open && depth == 0 {
                break;
            }
            j += 1;
        }
        out.push((name, body));
        i = j + 1;
    }
    out
}

/// Does this body run a statement and then never look at what it returned?
fn discards_row_count(body: &str) -> bool {
    if !body.contains(".execute(&mut conn)") {
        return false;
    }
    // Reading it looks like `let affected = ...execute(...)` or a `Result<usize>`
    // return type carrying it up to the caller.
    !body.contains("let affected") && !body.contains("Result<usize, DatabaseError>")
}

fn returns_unit(body: &str) -> bool {
    // The signature is on the first few lines of the captured body.
    body.lines()
        .take(8)
        .any(|l| l.contains("Result<(), DatabaseError>"))
}

#[test]
fn the_scan_finds_the_writers_this_check_is_about() {
    let source = read("server/src/database.rs");
    let found: Vec<String> = functions(&source)
        .into_iter()
        .filter(|(_, body)| returns_unit(body) && discards_row_count(body))
        .map(|(name, _)| name)
        .collect();

    // Anti-vacuity. A parser that stopped matching would make every assertion
    // below trivially true, so the scan is asserted to have found something of
    // roughly the expected size before it is compared to anything.
    assert!(
        found.len() >= 8,
        "the scan found only {} writers, which means the parser stopped working \
         rather than the code getting better. Fix the parser before trusting \
         this file. Found: {found:?}",
        found.len()
    );
}

#[test]
fn every_writer_that_ignores_its_row_count_is_listed_here() {
    let source = read("server/src/database.rs");
    let found: Vec<String> = functions(&source)
        .into_iter()
        .filter(|(_, body)| returns_unit(body) && discards_row_count(body))
        .map(|(name, _)| name)
        .collect();

    let listed: Vec<&str> = WRITERS.iter().map(|w| w.name).collect();

    let unlisted: Vec<&String> = found
        .iter()
        .filter(|n| !listed.contains(&n.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "these writers return `Result<(), DatabaseError>` and never read the row \
         count from `.execute()`, and are not listed in this check:\n  {unlisted:?}\n\n\
         That is the shape that made `remove_tool_trainer` answer 200 for \
         removing nothing and then audit it. Either read the count and return \
         `NotFound` when it is zero, or add the function here with a reason \
         saying why the count cannot matter."
    );

    let stale: Vec<&&str> = listed
        .iter()
        .filter(|n| !found.contains(&n.to_string()))
        .collect();
    assert!(
        stale.is_empty(),
        "these are listed here but no longer match the pattern:\n  {stale:?}\n\n\
         If they were fixed, delete them from WRITERS and lower \
         RECORDED_DEFECTS. If they were renamed or removed, update the list. \
         A list that names functions which no longer exist stops being a \
         record of anything."
    );
}

#[test]
fn the_number_of_recorded_defects_has_not_gone_up() {
    let defects = WRITERS.iter().filter(|w| w.exempt.is_none()).count();

    assert_eq!(
        defects, RECORDED_DEFECTS,
        "the number of writers that ignore a row count they should be reading \
         changed from {RECORDED_DEFECTS} to {defects}.\n\n\
         Down is good: lower RECORDED_DEFECTS and say which one was fixed. Up \
         means a new writer was added in a shape that has already produced two \
         production defects -- read the count and return NotFound when it is \
         zero."
    );
}

#[test]
fn every_exemption_says_why() {
    for w in WRITERS {
        if let Some(reason) = w.exempt {
            assert!(
                reason.len() > 40,
                "the exemption for `{}` is too short to be a reason. Every \
                 narrowing needs one that covers exactly what it narrows.",
                w.name
            );
        }
    }
}

#[test]
fn the_two_writers_already_fixed_stay_fixed() {
    let source = read("server/src/database.rs");
    let by_name: Vec<(String, String)> = functions(&source);

    /// Writers that were in the offending shape and have been fixed. A regression
    /// here is the same defect coming back, so it is a gate rather than part of
    /// the ratchet above.
    const ALREADY_FIXED: &[&str] = &["remove_tool_trainer"];

    for name in ALREADY_FIXED {
        let (_, body) = by_name
            .iter()
            .find(|(n, _)| n == *name)
            .unwrap_or_else(|| panic!("`{name}` is gone from database.rs"));
        assert!(
            body.contains("let affected"),
            "`{name}` stopped reading its row count. It was fixed on this branch \
             precisely because returning 200 for removing nothing, and then \
             writing an audit entry saying otherwise, is how an access-control \
             system loses track of who may open a door."
        );
    }
}
