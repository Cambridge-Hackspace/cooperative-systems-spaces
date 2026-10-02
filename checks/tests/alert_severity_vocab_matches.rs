//! The severity vocabulary lives in two places: the `CHECK (min_severity IN
//! (...))` on `webhook_class_subscriptions`, and `Severity::ALL` /
//! `Severity::as_str` in `server/src/models/alerts.rs`. Either alone can
//! drift -- a level added to the enum but not the CHECK compiles, migrates,
//! and fails at the first subscription that uses it. This duplicates the
//! mapping deliberately and asserts the two agree. Same shape as
//! `tool_module_vocab_matches.rs`; the migration is located by a needle so a
//! later redefinition supersedes this one.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

const MODELS: &str = "server/src/models/alerts.rs";

fn migration_defining(needle: &str) -> String {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut last = None;
    for d in dirs {
        if let Ok(sql) = std::fs::read_to_string(d.join("up.sql")) {
            if sql.contains(needle) {
                last = Some(sql);
            }
        }
    }
    last.unwrap_or_else(|| panic!("no migration contains {needle:?}"))
}

/// The literals inside `CHECK (<column> IN (...))`.
fn check_values(sql: &str, column: &str) -> BTreeSet<String> {
    let needle = format!("CHECK ({column} IN (");
    let start = sql
        .find(&needle)
        .unwrap_or_else(|| panic!("no CHECK on {column}"))
        + needle.len();
    let end = sql[start..].find("))").expect("CHECK closes") + start;
    sql[start..end]
        .split(',')
        .map(|s| s.trim().trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The `as_str` arms of `impl Severity`.
fn rust_severities() -> BTreeSet<String> {
    let src = read(MODELS);
    let start = src.find("impl Severity {").expect("impl Severity");
    let body = &src[start..];
    let end = body.find("\n}\n").expect("impl Severity closes");
    let body = &body[..end];
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Severity::") {
            if let Some((_, lit)) = rest.split_once("=> \"") {
                if let Some(end) = lit.find('"') {
                    out.insert(lit[..end].to_string());
                }
            }
        }
    }
    out
}

#[test]
fn the_check_and_the_enum_agree() {
    let sql = migration_defining("CREATE TABLE webhook_class_subscriptions");
    let check = check_values(&sql, "min_severity");
    let rust = rust_severities();
    assert!(check.len() >= 3, "parsed too few CHECK values: {check:?}");
    assert!(rust.len() >= 3, "parsed too few Rust severities: {rust:?}");
    assert_eq!(
        check, rust,
        "webhook_class_subscriptions.min_severity CHECK and Severity::as_str disagree"
    );
}

#[test]
fn the_oracle_discriminates() {
    // Both parsers must have actually found their shapes, or the equality
    // above would be an equality of two empty sets.
    assert!(rust_severities().contains("critical"));
    let sql = "CREATE TABLE x (min_severity TEXT NOT NULL CHECK (min_severity IN ('a', 'b')))";
    assert_eq!(
        check_values(sql, "min_severity"),
        ["a", "b"].into_iter().map(String::from).collect()
    );
}
