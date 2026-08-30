//! `AuditEventType::as_str` must be a subset of the audit_logs CHECK constraint.
//!
//! Every audit write in this codebase is `let _ = state.db.create_audit_log(..)`.
//! The result is discarded — deliberately, because failing a user's request
//! because the audit row would not insert is worse than losing the row. The
//! consequence is that a Rust variant whose string the CHECK constraint does not
//! list produces **no error anywhere**: the insert violates the constraint, the
//! error is dropped on the floor, the user's request succeeds, and the event is
//! simply never recorded. Nothing logs, nothing alerts, nothing fails.
//!
//! For most of these that is a gap in a report. For
//! `door_unlocked_card`, `door_unlock_denied` and `tool_access_denied` it is the
//! absence of a record that somebody opened a door or was refused a machine.
//!
//! The constraint has been redefined ten times across the migration history, and
//! each redefinition restates the *whole* list — so an event type dropped by a
//! later migration disappears without any migration mentioning it. That is the
//! specific accident this check exists for.
//!
//! Source-as-data on both sides: the enum's `as_str` arms, and the last
//! migration that redefines the constraint. Neither is derived from the other.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

/// The wire strings `AuditEventType::as_str` can return.
fn rust_event_types() -> BTreeSet<String> {
    let src = read("server/src/models.rs");
    let start = src
        .find("impl AuditEventType {")
        .expect("server/src/models.rs must define impl AuditEventType");
    let body = &src[start..];
    let end = body.find("\n}").unwrap_or(body.len());

    let mut out = BTreeSet::new();
    for line in body[..end].lines() {
        let code = line.split("//").next().unwrap_or("");
        // `Self::UserLogin => "user_login",`
        if let Some(arrow) = code.find("=>") {
            let rhs = &code[arrow + 2..];
            if let Some(open) = rhs.find('"') {
                let after = &rhs[open + 1..];
                if let Some(close) = after.find('"') {
                    out.insert(after[..close].to_string());
                }
            }
        }
    }
    out
}

/// The values the *last* migration to redefine the constraint allows.
///
/// "Last" is by directory name, which is diesel's own ordering. Reading only
/// the last one is correct and is the whole point: each redefinition replaces
/// the constraint outright, so the earlier lists describe schemas that no longer
/// exist.
fn constraint_event_types() -> (String, BTreeSet<String>) {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut last: Option<(String, String)> = None;
    for dir in dirs {
        let up = dir.join("up.sql");
        let Ok(text) = std::fs::read_to_string(&up) else {
            continue;
        };
        if text.contains("ADD CONSTRAINT audit_logs_event_type_check") {
            let name = dir
                .file_name()
                .expect("a directory has a name")
                .to_string_lossy()
                .to_string();
            last = Some((name, text));
        }
    }

    let (name, text) = last.expect(
        "no migration adds audit_logs_event_type_check; either the constraint is \
         gone or this scan is broken",
    );

    // Everything between `CHECK (event_type IN (` and the closing `))`.
    let at = text
        .find("CHECK (event_type IN (")
        .expect("the constraint is added without a value list");
    let body = &text[at..];
    let end = body.find("))").unwrap_or(body.len());

    let mut out = BTreeSet::new();
    for line in body[..end].lines() {
        let code = line.split("--").next().unwrap_or("");
        let mut rest = code;
        while let Some(open) = rest.find('\'') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('\'') else { break };
            out.insert(after[..close].to_string());
            rest = &after[close + 1..];
        }
    }
    (name, out)
}

#[test]
fn both_sides_were_actually_parsed() {
    // Either scan returning nothing would make the comparison below pass over
    // two empty sets -- which is the failure this whole file is about, wearing
    // the check's own clothes.
    let rust = rust_event_types();
    let (migration, sql) = constraint_event_types();

    assert!(
        rust.len() >= 60,
        "parsed only {} event types from AuditEventType::as_str; the scan is broken",
        rust.len()
    );
    assert!(
        sql.len() >= 20,
        "parsed only {} values from {migration}; the scan is broken",
        sql.len()
    );
}

#[test]
fn every_rust_event_type_is_allowed_by_the_constraint() {
    let rust = rust_event_types();
    let (migration, sql) = constraint_event_types();

    let rejected: Vec<&String> = rust.difference(&sql).collect();

    assert!(
        rejected.is_empty(),
        "these audit event types exist in Rust and are NOT in the CHECK constraint \
         defined by {migration}:\n{rejected:#?}\n\n\
         Writing one of these produces no error anybody sees. `create_audit_log` \
         is always called as `let _ = ...`, so the constraint violation is \
         discarded, the request succeeds, and the event is never recorded. \
         Several of these are door unlocks and tool refusals.\n\n\
         The fix is a new migration that redefines the constraint with the full \
         list -- not an edit to an applied one, which would leave every existing \
         deployment on the old constraint while the repository claimed otherwise."
    );
}

#[test]
fn the_constraint_allows_nothing_rust_cannot_produce() {
    // The other direction is a weaker claim and a real one: a value the
    // constraint permits and no code emits is either a feature that was removed
    // without cleaning up, or -- worse -- a variant somebody deleted from the
    // enum while rows carrying it still exist and are now unmappable.
    let rust = rust_event_types();
    let (migration, sql) = constraint_event_types();

    let orphaned: Vec<&String> = sql.difference(&rust).collect();

    assert!(
        orphaned.is_empty(),
        "{migration} permits these audit event types and no Rust variant emits \
         them:\n{orphaned:#?}\n\n\
         Either a variant was removed from AuditEventType without a migration \
         narrowing the constraint -- in which case existing rows carry a value \
         nothing can now interpret -- or the constraint was widened for an event \
         that was never implemented."
    );
}
