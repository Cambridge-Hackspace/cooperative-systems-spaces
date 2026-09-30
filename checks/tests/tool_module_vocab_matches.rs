//! The binding / interlock vocabularies live in two places: the
//! `CHECK (... IN (...))` constraints in the migrations, and the `pub const` sets
//! in `server/src/models/tool_modules.rs`. Either alone can drift -- a value added
//! to the CHECK but not the Rust consts (or vice versa) compiles and migrates
//! fine, and the mismatch only surfaces as a runtime insert error much later. This
//! oracle duplicates the mapping deliberately and asserts the two agree.
//!
//! **Each vocabulary is located rather than hardcoded.** They no longer share one
//! migration: #101 moved `role` and `on_disconnect` onto `device_bindings` (the
//! binding replaced `tool_modules`), and then widened `role` again with `edge` when
//! a door's coordinator became a binding -- while the interlock vocabularies stayed
//! where `tool_interlocks` was created. Pointing this file at a fixed migration
//! would mean reading a dropped table's historical CHECK and passing while the live
//! one differed, which is precisely the drift it exists to catch. So each entry
//! carries a needle that identifies the migration currently defining it, and the
//! LAST migration matching that needle wins -- a later redefinition supersedes an
//! earlier one, in diesel's own order.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

const MODELS: &str = "server/src/models/tool_modules.rs";

/// The contents of the last `up.sql`, in diesel's order, containing `needle`.
///
/// "Last" so a migration that redefines a CHECK supersedes the one that first
/// created it; the needle is a string distinctive to the statement that defines
/// the vocabulary, so this tracks the live definition instead of a path.
fn migration_defining(needle: &str) -> String {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut found: Option<String> = None;
    for dir in dirs {
        if let Ok(sql) = std::fs::read_to_string(dir.join("up.sql")) {
            if sql.contains(needle) {
                found = Some(sql);
            }
        }
    }
    found.unwrap_or_else(|| {
        panic!(
            "no migration up.sql contains `{needle}`, so the vocabulary it should \
             define cannot be located. Either the statement was reworded -- in which \
             case this needle needs updating -- or the definition is gone."
        )
    })
}

/// The single-quoted values inside the first `<col> IN ( ... )` in `sql`.
fn sql_check_values(sql: &str, col: &str) -> BTreeSet<String> {
    let needle = format!("{col} IN (");
    let start = sql
        .find(&needle)
        .unwrap_or_else(|| panic!("the located migration has no `{needle}`"))
        + needle.len();
    let end = start
        + sql[start..]
            .find(')')
            .expect("unterminated IN ( ... ) in migration");
    quoted(&sql[start..end], '\'')
}

/// The double-quoted string literals inside the `pub mod <name> { ... }` block.
fn rust_module_values(src: &str, module: &str) -> BTreeSet<String> {
    let needle = format!("pub mod {module} {{");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("models file has no `{needle}`"))
        + needle.len();
    // The module body ends at the first `}` that closes it. These modules contain
    // no nested braces, so the next `}` is the closer.
    let end = start + src[start..].find('}').expect("unterminated module block");
    quoted(&src[start..end], '"')
}

/// Every substring delimited by `q` in `s`.
fn quoted(s: &str, q: char) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == q {
            let mut val = String::new();
            for c2 in it.by_ref() {
                if c2 == q {
                    break;
                }
                val.push(c2);
            }
            if !val.is_empty() {
                out.insert(val);
            }
        }
    }
    out
}

/// (Rust module, SQL column, needle identifying the migration that defines it).
const VOCABULARIES: [(&str, &str, &str); 6] = [
    // Widened with `edge` by the door-coordinator fold, via a named constraint.
    ("binding_role", "role", "device_bindings_role_check"),
    // Unchanged since the binding table was created.
    (
        "on_disconnect",
        "on_disconnect",
        "CREATE TABLE device_bindings",
    ),
    // The interlock vocabularies stayed with the table they belong to.
    ("interlock_kind", "kind", "CREATE TABLE tool_interlocks"),
    (
        "interlock_condition",
        "condition",
        "CREATE TABLE tool_interlocks",
    ),
    ("interlock_reset", "reset", "CREATE TABLE tool_interlocks"),
    ("enforcement", "enforcement", "CREATE TABLE tool_interlocks"),
];

#[test]
fn binding_and_interlock_vocabularies_match_between_sql_and_rust() {
    let rust = read(MODELS);

    for (module, col, needle) in VOCABULARIES {
        let sql = migration_defining(needle);
        let from_sql = sql_check_values(&sql, col);
        let from_rust = rust_module_values(&rust, module);
        assert!(
            !from_sql.is_empty(),
            "no CHECK values parsed for column `{col}` out of the migration \
             defining `{needle}`; the scan is broken and the comparison below \
             would pass over an empty set"
        );
        assert_eq!(
            from_rust, from_sql,
            "vocabulary drift for `{module}` / SQL column `{col}` (defined by the \
             migration containing `{needle}`):\n  rust consts: {from_rust:?}\n  \
             sql CHECK:   {from_sql:?}"
        );
    }
}

/// The `role` vocabulary is now widened in a migration *later* than the one that
/// created the column, so this pins that the locator follows the redefinition
/// rather than the original. Without it, a regression to reading the creating
/// migration would still pass the test above for every other vocabulary and fail
/// only on `role` -- confusingly, and only by luck.
#[test]
fn the_role_locator_finds_the_widened_definition_not_the_original() {
    let sql = migration_defining("device_bindings_role_check");
    let roles = sql_check_values(&sql, "role");
    assert!(
        roles.contains("edge"),
        "the located `role` CHECK does not contain `edge`, so it is the pre-#101 \
         definition from the migration that created the column rather than the \
         one that widened it: {roles:?}"
    );
}
