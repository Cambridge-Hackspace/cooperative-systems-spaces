//! Every foreign key to `users` is known to the merge (#118).
//!
//! `server/src/user_merge.rs` re-points references by name, from four hand-kept
//! lists. A migration that adds a new `REFERENCES users(id)` without touching
//! those lists would make the merge silently leave rows behind -- orphaned by
//! the cascade, or anonymised by SET NULL, or refusing the delete under
//! RESTRICT. This derives the set of live `(table, column)` references from the
//! migrations themselves and holds the lists to it, in both directions.
//!
//! Derived from the migration SQL rather than from `schema.rs`, because Diesel
//! allows one `joinable!` per table pair and `audit_logs` alone has two
//! columns pointing at `users`.

use css_checks::repo_root;
use std::collections::BTreeSet;

/// `(table, column)` pairs that reference `users(id)` in the live schema.
fn references_from_migrations() -> BTreeSet<(String, String)> {
    let dir = repo_root().join("server/migrations");
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("server/migrations must exist")
        .map(|e| e.expect("readable entry").path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();

    let mut refs: BTreeSet<(String, String)> = BTreeSet::new();
    for migration in entries {
        let Ok(sql) = std::fs::read_to_string(migration.join("up.sql")) else {
            continue;
        };
        let mut current_table: Option<String> = None;
        for raw in sql.lines() {
            let line = raw.trim();
            if line.starts_with("--") {
                continue;
            }
            let upper = line.to_ascii_uppercase();
            if let Some(rest) = upper.strip_prefix("CREATE TABLE ") {
                let rest = rest.trim_start_matches("IF NOT EXISTS ");
                let name = rest
                    .split(|c: char| c == '(' || c.is_whitespace())
                    .next()
                    .unwrap_or("");
                current_table = Some(name.to_ascii_lowercase());
            }
            if upper.starts_with("DROP TABLE") {
                let name = upper
                    .trim_start_matches("DROP TABLE")
                    .trim()
                    .trim_start_matches("IF EXISTS")
                    .trim()
                    .split(|c: char| c == ';' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                refs.retain(|(t, _)| t != &name);
                continue;
            }
            if let Some(rest) = upper.strip_prefix("ALTER TABLE ") {
                let mut words = rest.split_whitespace();
                let table = words.next().unwrap_or("").to_ascii_lowercase();
                let rest: Vec<&str> = words.collect();
                if rest.first() == Some(&"DROP") && rest.get(1) == Some(&"COLUMN") {
                    let col = rest
                        .get(2)
                        .unwrap_or(&"")
                        .trim_end_matches(';')
                        .to_ascii_lowercase();
                    refs.remove(&(table, col));
                    continue;
                }
                if rest.first() == Some(&"ADD")
                    && rest.get(1) == Some(&"COLUMN")
                    && upper.contains("REFERENCES USERS")
                {
                    let col = rest.get(2).unwrap_or(&"").to_ascii_lowercase();
                    refs.insert((table, col));
                }
                continue;
            }
            if upper.contains("REFERENCES USERS") {
                if let Some(table) = &current_table {
                    let col = line
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    refs.insert((table.clone(), col));
                }
            }
            if line.ends_with(");") {
                current_table = None;
            }
        }
    }
    refs
}

/// Every `(...)` tuple in a slice literal, as the quoted strings it holds, in
/// order. rustfmt may put a tuple on one line or several; this reads the
/// parentheses, not the lines.
fn tuples_in(list_src: &str) -> Vec<Vec<String>> {
    let mut tuples = Vec::new();
    let mut current: Option<Vec<String>> = None;
    let mut chars = list_src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' => current = Some(Vec::new()),
            ')' => {
                if let Some(t) = current.take() {
                    tuples.push(t);
                }
            }
            '"' => {
                let mut s = String::new();
                for d in chars.by_ref() {
                    if d == '"' {
                        break;
                    }
                    s.push(d);
                }
                if let Some(t) = current.as_mut() {
                    t.push(s);
                }
            }
            _ => {}
        }
    }
    tuples
}

/// The union of the lists in user_merge.rs, read from the source.
fn references_the_merge_knows() -> BTreeSet<(String, String)> {
    let src = std::fs::read_to_string(repo_root().join("server/src/user_merge.rs"))
        .expect("server/src/user_merge.rs must exist");
    let mut out = BTreeSet::new();
    for list in ["PLAIN", "UNIQUE_PAIRS", "SPECIAL", "DROPPED"] {
        let start = src
            .find(&format!("pub const {list}:"))
            .unwrap_or_else(|| panic!("user_merge.rs no longer defines {list}"));
        // `= &[`, not the first `&[`: the type annotation `&[(&str, &str)]`
        // comes first on the line and would read as a tuple of no strings.
        let open = src[start..]
            .find("= &[")
            .map(|i| start + i + 2)
            .expect("list start");
        let end = src[open..].find("];").map(|i| open + i).expect("list end");
        for tuple in tuples_in(&src[open..end]) {
            assert!(
                tuple.len() >= 2,
                "{list} holds a tuple without a table and a column: {tuple:?}"
            );
            // UNIQUE_PAIRS name the key column; the user column is user_id.
            let col = if list == "UNIQUE_PAIRS" {
                "user_id".to_string()
            } else {
                tuple[1].clone()
            };
            out.insert((tuple[0].clone(), col));
        }
    }
    out
}

#[test]
fn the_scan_discriminates() {
    // Upward: the migration scan must have found the shape it exists to find.
    let found = references_from_migrations();
    assert!(
        found.len() >= 30,
        "only {} references found; the scan is broken",
        found.len()
    );
    for (t, c) in [
        ("audit_logs", "actor_id"),
        ("tools", "created_by"),
        ("user_cards", "user_id"),
    ] {
        assert!(
            found.contains(&(t.to_string(), c.to_string())),
            "{t}.{c} missing from the scan"
        );
    }
    // Downward: a table dropped later is not reported. training_records was
    // created with two user references and dropped in 2026-09-13.
    assert!(
        !found.iter().any(|(t, _)| t == "training_records"),
        "the scan reports a dropped table; DROP TABLE handling is broken"
    );
}

#[test]
fn every_user_reference_is_handled_by_the_merge() {
    let found = references_from_migrations();
    let known = references_the_merge_knows();
    let unknown: Vec<_> = found.difference(&known).collect();
    assert!(
        unknown.is_empty(),
        "these references to users(id) are not in any list in server/src/user_merge.rs, \
         so a merge would leave them behind: {unknown:?}. Add each to PLAIN, UNIQUE_PAIRS, \
         SPECIAL or DROPPED, with the handling it needs."
    );
    let stale: Vec<_> = known.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "user_merge.rs lists references the migrations no longer define: {stale:?}"
    );
}
