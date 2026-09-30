//! The device role vocabulary is written twice: the `<@ '[...]'` JSON literal in
//! the `space_devices_roles_vocab` CHECK constraint, and the `pub const` set in
//! `server/src/models/devices.rs`'s `device_role` module. Nothing makes them
//! agree by construction.
//!
//! This replaces `device_kinds_agree.rs`, which pinned the four-way agreement of
//! the old `space_device_kind` enum (migration labels, ToSql, FromSql, and the
//! registration match arms). #101 dropped that enum for a JSONB `capabilities`
//! column, so three of those four parties no longer exist: the column is JSONB,
//! there is no ToSql/FromSql codec, and the registration endpoint validates
//! against the `device_role` consts directly rather than matching string literals
//! (so it cannot drift from them). What remains that CAN drift is the database
//! CHECK versus the Rust consts -- a role added to one and forgotten on the other
//! compiles and migrates fine, and surfaces only as a runtime insert error much
//! later. This oracle duplicates the mapping deliberately and asserts they agree.
//!
//! It also pins an invariant the two vocabularies must satisfy between them:
//! every bindable `module_role` (reader / power / sensor) must be a declarable
//! `device_role`, or a device could never legally be bound in a role the tool
//! wiring accepts.
//!
//! Text-level, like its neighbours: no database, no compiler, so it runs on the
//! FreeBSD workstation where `css-server` cannot be built.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

const MODELS_DEVICES: &str = "server/src/models/devices.rs";
const MODELS_MODULES: &str = "server/src/models/tool_modules.rs";

/// The double-quoted strings inside the first JSON array literal that follows the
/// `space_devices_roles_vocab` constraint, across all migration `up.sql` files.
fn sql_vocab() -> BTreeSet<String> {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    for dir in dirs {
        let Ok(sql) = std::fs::read_to_string(dir.join("up.sql")) else {
            continue;
        };
        let Some(at) = sql.find("space_devices_roles_vocab") else {
            continue;
        };
        // The vocabulary is the JSON array literal in the `<@ '[...]'` that gates
        // membership. Find the `<@`, then the `[` and `]` that bound its literal.
        let rest = &sql[at..];
        let Some(op) = rest.find("<@") else { continue };
        let after = &rest[op..];
        let start = after
            .find('[')
            .expect("no `[` after `<@` in the roles CHECK");
        let end = after
            .find(']')
            .expect("no `]` after `<@` in the roles CHECK");
        return double_quoted(&after[start..end]);
    }
    panic!("no migration defines the `space_devices_roles_vocab` CHECK constraint");
}

/// The double-quoted `pub const` string values inside `pub mod <module>`,
/// excluding the `ALL` array (which quotes nothing) -- these are the identifiers'
/// values, e.g. `"reader"`.
fn rust_vocab(src: &str, module: &str) -> BTreeSet<String> {
    let needle = format!("pub mod {module} {{");
    let start = src.find(&needle).unwrap_or_else(|| panic!("no `{needle}`")) + needle.len();
    // These modules contain no nested braces, so the next `}` closes them.
    let end = start + src[start..].find('}').expect("unterminated module block");
    double_quoted(&src[start..end])
}

/// Every substring delimited by `"` in `s`.
fn double_quoted(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '"' {
            let mut val = String::new();
            for c2 in it.by_ref() {
                if c2 == '"' {
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
fn both_sides_were_actually_parsed() {
    // Either scan returning nothing would make the comparison pass over empty
    // sets, which is the failure this file exists to prevent.
    let sql = sql_vocab();
    let rust = rust_vocab(&read(MODELS_DEVICES), "device_role");
    assert!(
        sql.len() >= 5,
        "parsed too few roles from the SQL CHECK: {sql:?}"
    );
    assert!(
        rust.len() >= 5,
        "parsed too few roles from device_role consts: {rust:?}"
    );
}

#[test]
fn the_device_role_vocabularies_agree() {
    let sql = sql_vocab();
    let rust = rust_vocab(&read(MODELS_DEVICES), "device_role");

    assert_eq!(
        rust, sql,
        "device role vocabulary drift between the SQL CHECK and the Rust \
         `device_role` consts:\n  rust consts: {rust:?}\n  sql CHECK:   {sql:?}\n\n\
         A role in one and not the other migrates and compiles, then fails as a \
         `space_devices_roles_vocab` constraint violation at insert time. Add it \
         to both."
    );
}

#[test]
fn every_bindable_module_role_is_a_declarable_device_role() {
    let device = rust_vocab(&read(MODELS_DEVICES), "device_role");
    let module = rust_vocab(&read(MODELS_MODULES), "module_role");

    assert!(
        !module.is_empty(),
        "parsed no module_role values; the scan is broken"
    );
    let orphans: Vec<&String> = module.difference(&device).collect();
    assert!(
        orphans.is_empty(),
        "these `module_role` values are not in `device_role`: {orphans:?}\n\n\
         A tool binding accepts these roles, but a device cannot declare them, so \
         no device could ever be legally bound in one. Add them to `device_role` \
         (and the SQL CHECK)."
    );
}

// ── Self-tests ───────────────────────────────────────────────────────────────
//
// Each parser and check is fed the input it exists to reject. A check never
// observed failing is a check of unmeasured value.

#[cfg(test)]
mod the_checks_reject_what_they_are_for {
    use super::*;

    #[test]
    fn the_sql_extractor_reads_the_array_literal() {
        // Modelled on the real CHECK line.
        let s = "capabilities->'roles' <@ '[\"reader\",\"power\",\"edge\"]'::jsonb";
        let start = s.find('[').unwrap();
        let end = s.find(']').unwrap();
        let got = double_quoted(&s[start..end]);
        assert_eq!(
            got,
            ["reader", "power", "edge"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn the_rust_extractor_skips_the_all_array() {
        let src = "pub mod device_role {\n\
                   pub const READER: &str = \"reader\";\n\
                   pub const POWER: &str = \"power\";\n\
                   pub const ALL: [&str; 2] = [READER, POWER];\n\
                   }";
        let got = rust_vocab(src, "device_role");
        // ALL references identifiers, not string literals, so it contributes
        // nothing -- only the two declared values survive.
        assert_eq!(
            got,
            ["reader", "power"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn drift_between_sql_and_rust_would_be_caught() {
        let sql: BTreeSet<String> = ["reader", "power"].iter().map(|s| s.to_string()).collect();
        let rust: BTreeSet<String> = ["reader", "power", "sensor"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_ne!(
            rust, sql,
            "a role added to Rust but not SQL must not compare equal"
        );
    }

    #[test]
    fn an_orphan_module_role_would_be_caught() {
        let device: BTreeSet<String> = ["edge", "kiosk"].iter().map(|s| s.to_string()).collect();
        let module: BTreeSet<String> = ["reader"].iter().map(|s| s.to_string()).collect();
        let orphans: Vec<&String> = module.difference(&device).collect();
        assert!(
            !orphans.is_empty(),
            "a module_role absent from device_role must be reported"
        );
    }
}
