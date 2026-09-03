//! `server/src/schema.rs` must describe every table the migrations create.
//!
//! There is no `diesel.toml` in this repository, so `schema.rs` is not
//! regenerated from the database -- it is maintained by hand, and nothing has
//! ever compared it to the migrations. A table can therefore exist in Postgres
//! and be absent from the file that is supposed to describe Postgres, and the
//! workspace still compiles, because nothing referred to the missing table in
//! the first place.
//!
//! That is not a hypothetical. `audit_event_types` was added by
//! `2026-09-02-120000-0000_replace_audit_event_check_with_lookup_table` and
//! left out of `schema.rs`, on the reasoning that no Rust code queries it --
//! the foreign key is enforced entirely by the database. The reasoning was
//! right and the conclusion was wrong: `e2e/run.sh` names the tables its later
//! stages depend on and says in a comment that it takes those names *from
//! schema.rs*, so a table missing here is a table nothing asserts the existence
//! of, anywhere, at any tier.
//!
//! The cost of the omission is bounded and the cost of the check is one file,
//! so this exists rather than a note asking the next person to remember.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

/// Tables created by migrations, less any a later migration dropped.
///
/// Visited in diesel's order (by directory name) so a table that was created
/// and later dropped does not count as live.
fn migration_tables() -> BTreeSet<String> {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut live = BTreeSet::new();
    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join("up.sql")) else {
            continue;
        };
        for (verb, creates) in [("CREATE TABLE", true), ("DROP TABLE", false)] {
            for name in statements_naming(&text, verb) {
                if creates {
                    live.insert(name);
                } else {
                    live.remove(&name);
                }
            }
        }
    }
    live
}

/// The table names following each `verb` in `text`, skipping the SQL noise
/// words that may sit between the verb and the name.
fn statements_naming(text: &str, verb: &str) -> Vec<String> {
    let upper = text.to_uppercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = upper[from..].find(verb) {
        let at = from + rel;
        from = at + verb.len();
        // `CREATE TABLE IF NOT EXISTS foo (` / `DROP TABLE IF EXISTS foo;`
        let tail = &text[from..];
        let name = tail
            .split_whitespace()
            .find(|w| {
                !matches!(
                    w.to_uppercase().as_str(),
                    "IF" | "NOT" | "EXISTS" | "ONLY" | "TEMP" | "TEMPORARY" | "UNLOGGED"
                )
            })
            .unwrap_or("");
        let name: String = name
            .trim_start_matches('"')
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push(name.to_lowercase());
        }
    }
    out
}

/// The tables `schema.rs` declares, one per `diesel::table!` invocation.
fn declared_tables() -> BTreeSet<String> {
    let src = read("server/src/schema.rs");
    let mut out = BTreeSet::new();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find("diesel::table! {") {
        let at = from + rel;
        from = at + "diesel::table! {".len();
        // The body opens with `name (primary_key) {`, except where the table
        // has a custom SQL type -- those blocks lead with one or more `use`
        // lines, and reading the first line blindly records `use` as the name
        // of six real tables while hiding their absence.
        let Some(line) = src[from..]
            .lines()
            .skip(1)
            .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("use "))
        else {
            continue;
        };
        let name: String = line
            .trim()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.insert(name);
        }
    }
    out
}

#[test]
fn both_sides_were_actually_parsed() {
    // Either scan returning nothing would make the comparisons below pass over
    // empty sets, which is the failure this file exists to prevent.
    let migrations = migration_tables();
    let declared = declared_tables();
    assert!(
        migrations.len() >= 20,
        "parsed only {} tables out of the migrations; the scan is broken",
        migrations.len()
    );
    assert!(
        declared.len() >= 20,
        "parsed only {} tables out of schema.rs; the scan is broken",
        declared.len()
    );
}

#[test]
fn schema_rs_declares_every_table_the_migrations_create() {
    let migrations = migration_tables();
    let declared = declared_tables();

    let missing: Vec<&String> = migrations.difference(&declared).collect();

    assert!(
        missing.is_empty(),
        "these tables are created by a migration and absent from \
         server/src/schema.rs:\n{missing:#?}\n\n\
         schema.rs is hand-maintained here -- there is no diesel.toml -- so \
         nothing regenerates it and nothing else compares the two. A table \
         missing from it compiles fine, because no Rust code names it, and is \
         then invisible to e2e/run.sh's schema stage, which takes its table \
         list from this file by design. Add a `diesel::table!` block and the \
         name to `allow_tables_to_appear_in_same_query!`."
    );
}

#[test]
fn schema_rs_declares_nothing_the_migrations_do_not_create() {
    let migrations = migration_tables();
    let declared = declared_tables();

    let extra: Vec<&String> = declared.difference(&migrations).collect();

    assert!(
        extra.is_empty(),
        "server/src/schema.rs declares these, and no migration creates \
         them:\n{extra:#?}\n\n\
         Either a table was dropped without removing its block, or the block \
         was written for a table that was never migrated. Diesel will happily \
         build queries against either, and they will fail at runtime against \
         a real database."
    );
}
