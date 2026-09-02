//! `AuditEventType` must agree with the audit_logs event-type lookup table.
//!
//! Every audit write in this codebase is `let _ = state.db.create_audit_log(..)`.
//! The result is discarded — deliberately, because failing a user's request
//! because the audit row would not insert is worse than losing the row. The
//! consequence is that a Rust variant the database will not accept produces
//! **no error anywhere**: the insert is rejected, the error is dropped on the
//! floor, the user's request succeeds, and the event is simply never recorded.
//! Nothing logs, nothing alerts, nothing fails.
//!
//! For most of these that is a gap in a report. For `door_unlocked_card`,
//! `door_unlock_denied` and `tool_access_denied` it is the absence of a record
//! that somebody opened a door or was refused a machine.
//!
//! ## Why this reads inserted rows and not a CHECK constraint
//!
//! Until `2026-09-02-120000-0000_replace_audit_event_check_with_lookup_table`
//! the permitted set was a CHECK constraint that each migration restated *in
//! full*, and this file read the lexicographically last migration to restate
//! it. That was correct but fragile in a specific way: two branches could each
//! add an event type, merge with no textual conflict, and produce a database
//! that silently forbade whichever one lost the ordering. It came within one
//! merge of happening.
//!
//! The permitted set is now the union of every row ever inserted into
//! `audit_event_types`, which is **order-independent** — a merge cannot lose a
//! value it does not have to restate. This scan is therefore a union across all
//! migrations rather than a read of the last one.
//!
//! Source-as-data on both sides: the enum's own arms, and the migrations'
//! own SQL. Neither is derived from the other.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

/// Every `'literal'` in a slice of SQL, ignoring `--` comments.
fn sql_literals(fragment: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in fragment.lines() {
        let code = line.split("--").next().unwrap_or("");
        let mut rest = code;
        while let Some(open) = rest.find('\'') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('\'') else { break };
            out.insert(after[..close].to_string());
            rest = &after[close + 1..];
        }
    }
    out
}

/// The statement starting at `at`, up to its terminating semicolon.
fn statement_at(text: &str, at: usize) -> &str {
    let body = &text[at..];
    let end = body.find(';').map(|i| i + 1).unwrap_or(body.len());
    &body[..end]
}

/// The event types the database will accept: every row inserted into
/// `audit_event_types` by any migration, less any a later migration deleted.
///
/// Migrations are visited in diesel's own order (by directory name) so that a
/// deletion is applied after the insertion it retires. Within a single file,
/// inserts are applied before deletes; a migration that deleted a name and then
/// re-inserted it in the same file would be mismodelled here, and would also be
/// a very strange thing to write.
fn registered_event_types() -> (usize, BTreeSet<String>) {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut registered = BTreeSet::new();
    let mut touching = 0usize;

    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join("up.sql")) else {
            continue;
        };
        if !text.contains("audit_event_types") {
            continue;
        }
        touching += 1;

        let mut inserted = BTreeSet::new();
        let mut deleted = BTreeSet::new();
        for (marker, bucket) in [
            ("INSERT INTO audit_event_types", &mut inserted),
            ("DELETE FROM audit_event_types", &mut deleted),
        ] {
            let mut from = 0usize;
            while let Some(rel) = text[from..].find(marker) {
                let at = from + rel;
                bucket.extend(sql_literals(statement_at(&text, at)));
                from = at + marker.len();
            }
        }

        registered.extend(inserted);
        for name in &deleted {
            registered.remove(name);
        }
    }

    (touching, registered)
}

/// The `Self::Variant` names `as_str` has an arm for, paired with the wire
/// string each returns.
fn as_str_arms() -> Vec<(String, String)> {
    let src = read("server/src/models.rs");
    let start = src
        .find("pub fn as_str")
        .expect("server/src/models.rs must define AuditEventType::as_str");
    let end = src[start..]
        .find("pub fn all()")
        .expect("AuditEventType::all must follow as_str; this scan slices between them");
    let body = &src[start..start + end];

    let mut out = Vec::new();
    for line in body.lines() {
        let code = line.split("//").next().unwrap_or("");
        // `Self::UserLogin => "user_login",`
        let Some(arrow) = code.find("=>") else {
            continue;
        };
        let Some(variant) = code.split("Self::").nth(1) else {
            continue;
        };
        let variant: String = variant
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let rhs = &code[arrow + 2..];
        let Some(open) = rhs.find('"') else { continue };
        let after = &rhs[open + 1..];
        let Some(close) = after.find('"') else {
            continue;
        };
        if !variant.is_empty() {
            out.push((variant, after[..close].to_string()));
        }
    }
    out
}

/// The wire strings `AuditEventType::as_str` can return.
fn rust_event_types() -> BTreeSet<String> {
    as_str_arms().into_iter().map(|(_, wire)| wire).collect()
}

/// The variants listed in `AuditEventType::all()`.
///
/// `all()` opens with `use AuditEventType::*;`, so the entries are bare
/// identifiers rather than `Self::`-qualified ones.
fn enumerated_variants() -> BTreeSet<String> {
    let src = read("server/src/models.rs");
    let start = src
        .find("pub fn all()")
        .expect("server/src/models.rs must define AuditEventType::all");
    let body = &src[start..];
    let open = body.find("&[").expect("all() must return a slice literal");
    let close = body[open..]
        .find(']')
        .expect("the slice literal in all() must be closed");

    let mut out = BTreeSet::new();
    for line in body[open + 2..open + close].lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        let name: String = code
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && name.starts_with(|c: char| c.is_ascii_uppercase()) {
            out.insert(name);
        }
    }
    out
}

#[test]
fn both_sides_were_actually_parsed() {
    // Either scan returning nothing would make the comparisons below pass over
    // two empty sets -- which is the failure this whole file is about, wearing
    // the check's own clothes.
    let rust = rust_event_types();
    let (touching, registered) = registered_event_types();

    assert!(
        rust.len() >= 60,
        "parsed only {} event types from AuditEventType::as_str; the scan is broken",
        rust.len()
    );
    assert!(
        registered.len() >= 60,
        "parsed only {} rows from migrations that touch audit_event_types; \
         the scan is broken",
        registered.len()
    );
    assert!(
        touching >= 1,
        "no migration mentions audit_event_types; either the lookup table is \
         gone or this scan is broken"
    );
    assert!(
        !enumerated_variants().is_empty(),
        "parsed no variants out of AuditEventType::all(); the scan is broken"
    );
}

#[test]
fn every_rust_event_type_is_registered() {
    let rust = rust_event_types();
    let (_, registered) = registered_event_types();

    let rejected: Vec<&String> = rust.difference(&registered).collect();

    assert!(
        rejected.is_empty(),
        "these audit event types exist in Rust and are NOT rows in \
         audit_event_types:\n{rejected:#?}\n\n\
         Writing one of these produces no error anybody sees. `create_audit_log` \
         is always called as `let _ = ...`, so the foreign-key violation is \
         discarded, the request succeeds, and the event is never recorded. \
         Several of these are door unlocks and tool refusals.\n\n\
         The fix is a new migration containing \
         `INSERT INTO audit_event_types (name) VALUES ('the_new_type');` -- one \
         line, and no need to restate anything. Do not edit an applied \
         migration: that would leave every existing deployment on the old set \
         while the repository claimed otherwise."
    );
}

#[test]
fn the_table_registers_nothing_rust_cannot_produce() {
    // The other direction is a weaker claim and a real one: a value the table
    // permits and no code emits is either a feature that was removed without
    // cleaning up, or -- worse -- a variant somebody deleted from the enum
    // while rows carrying it still exist and are now unmappable.
    let rust = rust_event_types();
    let (_, registered) = registered_event_types();

    let orphaned: Vec<&String> = registered.difference(&rust).collect();

    assert!(
        orphaned.is_empty(),
        "audit_event_types registers these, and no AuditEventType variant \
         produces them:\n{orphaned:#?}\n\n\
         Either a variant was deleted from the enum without retiring its row, \
         or the row was inserted for a variant that was never added. If the \
         event type is genuinely gone, retire it with \
         `DELETE FROM audit_event_types WHERE name = '...';` in a new \
         migration -- but check first whether audit_logs still holds rows \
         referencing it, because the foreign key will refuse the delete if so, \
         and those rows are the records this table exists to keep."
    );
}

#[test]
fn every_event_type_is_enumerated_by_all() {
    // The third list. `as_str` decides what gets written; `all()` decides what
    // a webhook may subscribe to and what `validate_event_types` accepts. They
    // are maintained by hand, in the same impl block, and nothing until now
    // compared them.
    //
    // A variant present in `as_str` but missing from `all()` is not a crash: it
    // writes audit rows perfectly well while being unsubscribable by webhooks
    // *and* rejected as an unknown type if an operator names it explicitly. The
    // two halves fail in opposite directions, which is exactly why neither is
    // noticed.
    let arms = as_str_arms();
    let enumerated = enumerated_variants();

    let missing: Vec<&str> = arms
        .iter()
        .filter(|(variant, _)| !enumerated.contains(variant))
        .map(|(variant, _)| variant.as_str())
        .collect();

    assert!(
        missing.is_empty(),
        "these AuditEventType variants have an as_str arm but are missing from \
         all():\n{missing:#?}\n\n\
         They write audit rows, but no webhook can subscribe to them and \
         validate_event_types rejects them by name. Add them to all()."
    );
}

#[test]
fn all_enumerates_nothing_without_a_wire_string() {
    let arms = as_str_arms();
    let known: BTreeSet<&str> = arms.iter().map(|(v, _)| v.as_str()).collect();
    let enumerated = enumerated_variants();
    let extra: Vec<&String> = enumerated
        .iter()
        .filter(|v| !known.contains(v.as_str()))
        .collect();

    assert!(
        extra.is_empty(),
        "all() lists these, and as_str has no arm for them:\n{extra:#?}\n\n\
         If as_str is a non-exhaustive match this would not compile, so the \
         likely cause is that this scan's slicing of models.rs has drifted."
    );
}
