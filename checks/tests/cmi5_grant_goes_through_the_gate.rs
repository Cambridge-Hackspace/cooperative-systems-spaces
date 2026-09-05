//! A cmi5 pass grants tool access only through the shared training-completion
//! path — never by writing the access rows directly.
//!
//! Physical tool access is decided by `can_access_tool`, which reads
//! `user_training_progress` (Completed + unexpired) for a tool's steps, and the
//! toolguard sync path reads the same via a sibling query; `tool_access_agrees`
//! pins the two halves together. The trainer sign-off path writes those rows
//! through `create_training_record`, which upserts the progress row and records
//! a training_records entry, and its caller then broadcasts the new state to
//! edge devices.
//!
//! If the cmi5 grant instead did a raw `insert_into(user_training_progress)` or
//! touched `user_tool_training`, it would (a) skip the training_records audit
//! trail, (b) risk diverging from the shape `can_access_tool` expects, and
//! (c) most importantly bypass the broadcast, so a member could pass a course
//! and the door would not know. This check forbids that: the cmi5 service must
//! reach access only via `create_training_record`, and the grant must be
//! followed by a toolguard broadcast.
//!
//! Text-level, so it runs without a database or the compiler.

use css_checks::read;

/// The cmi5 service, with string literals and comments stripped, so a mention of
/// a table name in prose or a doc-comment cannot trip (or excuse) the check.
fn cmi5_service_code() -> String {
    strip(&read("server/src/cmi5.rs"))
}

/// Remove `//` line comments and `"..."` string literals from Rust source,
/// leaving code tokens. Crude but sufficient: it means the assertions below fire
/// on real Diesel calls, not on a table name written in a comment.
fn strip(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or("");
        let mut in_str = false;
        let mut prev = '\0';
        for c in code.chars() {
            if c == '"' && prev != '\\' {
                in_str = !in_str;
                prev = c;
                continue;
            }
            if !in_str {
                out.push(c);
            }
            prev = c;
        }
        out.push('\n');
    }
    out
}

#[test]
fn the_grant_goes_through_create_training_record() {
    let code = cmi5_service_code();
    assert!(
        code.contains("create_training_record"),
        "server/src/cmi5.rs no longer calls create_training_record. If the grant \
         path changed, it must still write access through a shared function that \
         upserts user_training_progress and records a training_records row — not \
         by hand — or the web and edge access checks can diverge and the audit \
         trail is lost."
    );
}

#[test]
fn the_cmi5_service_never_writes_the_access_tables_directly() {
    let code = cmi5_service_code();
    // A raw write to either table would bypass the gate. We look for the table
    // names appearing as Diesel query targets; the only legitimate mention of
    // user_training_progress is inside create_training_record, which lives in
    // database.rs, not here.
    for table in ["user_training_progress", "user_tool_training"] {
        assert!(
            !code.contains(table),
            "server/src/cmi5.rs references `{table}` directly. The cmi5 grant \
             must go through create_training_record, not touch the access tables \
             itself — a direct write skips the training_records audit row and, \
             worse, the toolguard broadcast, so a pass would not open the door."
        );
    }
}

#[test]
fn a_grant_is_followed_by_a_toolguard_broadcast() {
    // The broadcast lives in the handler (it needs &AppState). The grant handler
    // must call it, or an edge device never learns access changed.
    let handler = read("server/src/api/cmi5.rs");
    assert!(
        handler.contains("broadcast_toolguard_state"),
        "api/cmi5.rs does not broadcast toolguard state after a grant; a cmi5 \
         pass would update the database but never tell the edge devices."
    );
    assert!(
        handler.contains("Cmi5AuSatisfied"),
        "a cmi5 grant is not audited as Cmi5AuSatisfied; the record that a \
         browser course led to tool access would be missing."
    );
}
