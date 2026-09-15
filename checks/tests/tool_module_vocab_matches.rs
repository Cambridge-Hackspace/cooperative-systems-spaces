//! The tool_modules / tool_interlocks vocabularies live in two places: the
//! `CHECK (... IN (...))` constraints in the migration, and the `pub const` sets in
//! `server/src/models/tool_modules.rs`. Either alone can drift — a value added to
//! the CHECK but not the Rust consts (or vice versa) compiles and migrates fine, and
//! the mismatch only surfaces as a runtime insert error much later. This oracle
//! duplicates the mapping deliberately and asserts the two agree, so the drift is
//! caught here instead.

use css_checks::read;
use std::collections::BTreeSet;

const MIGRATION: &str = "server/migrations/2026-09-15-120000-0000_add_tool_modules/up.sql";
const MODELS: &str = "server/src/models/tool_modules.rs";

/// The single-quoted values inside the first `<col> IN ( ... )` in `sql`.
fn sql_check_values(sql: &str, col: &str) -> BTreeSet<String> {
    let needle = format!("{col} IN (");
    let start = sql
        .find(&needle)
        .unwrap_or_else(|| panic!("migration has no `{needle}`"))
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

#[test]
fn tool_module_vocabularies_match_between_sql_and_rust() {
    let sql = read(MIGRATION);
    let rust = read(MODELS);

    // (Rust module, SQL column) pairs that must describe the same value set.
    let pairs = [
        ("module_role", "role"),
        ("on_disconnect", "on_disconnect"),
        ("interlock_kind", "kind"),
        ("interlock_condition", "condition"),
        ("interlock_reset", "reset"),
        ("enforcement", "enforcement"),
    ];

    for (module, col) in pairs {
        let from_sql = sql_check_values(&sql, col);
        let from_rust = rust_module_values(&rust, module);
        assert!(
            !from_sql.is_empty(),
            "no CHECK values parsed for column `{col}`"
        );
        assert_eq!(
            from_rust, from_sql,
            "vocabulary drift for `{module}` / SQL column `{col}`:\n  \
             rust consts: {from_rust:?}\n  sql CHECK:   {from_sql:?}"
        );
    }
}
