//! The `space_device_kind` vocabulary is written out four times: the Postgres
//! enum labels in the migrations, the `ToSql` arms that write those labels, the
//! `FromSql` arms that read them, and the strings the registration endpoint
//! accepts. Nothing makes them agree.
//!
//! Each drift fails differently and late: a `ToSql` arm writing a label Postgres
//! does not have fails at INSERT time; a missing `FromSql` arm turns a legitimate
//! row into "Unrecognized enum variant" on read; a kind absent from the
//! registration match is simply un-registerable with no compile error anywhere.
//! This oracle duplicates the mapping deliberately and asserts all four agree.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

const MODELS: &str = "server/src/models/devices.rs";
const API: &str = "server/src/api/devices.rs";

/// The final set of `space_device_kind` labels after every migration, walked in
/// diesel's order so a later `CREATE TYPE` supersedes an earlier one.
fn migration_labels() -> BTreeSet<String> {
    let root = repo_root().join("server/migrations");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .expect("server/migrations must exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut labels = BTreeSet::new();
    for dir in dirs {
        let up = dir.join("up.sql");
        let Ok(sql) = std::fs::read_to_string(&up) else {
            continue;
        };
        let needle = "CREATE TYPE space_device_kind AS ENUM (";
        let Some(at) = sql.find(needle) else {
            continue;
        };
        let start = at + needle.len();
        let end = start + sql[start..].find(')').expect("unterminated CREATE TYPE");
        labels = single_quoted(&sql[start..end]);
    }
    labels
}

/// The `b"..."` byte strings inside the impl block whose header contains `header`,
/// bounded by the next `impl` item.
fn byte_strings_in_impl(src: &str, header: &str) -> BTreeSet<String> {
    let at = src
        .find(header)
        .unwrap_or_else(|| panic!("{MODELS} has no impl matching `{header}`"));
    let rest = &src[at + header.len()..];
    let end = rest.find("\nimpl ").unwrap_or(rest.len());
    let block = &rest[..end];

    let mut out = BTreeSet::new();
    let mut from = 0;
    while let Some(rel) = block[from..].find("b\"") {
        let s = from + rel + 2;
        let e = s + block[s..].find('"').expect("unterminated byte string");
        out.insert(block[s..e].to_string());
        from = e + 1;
    }
    out
}

/// The double-quoted match arms mapping a string to a `SpaceDeviceKind` variant.
fn api_accepted_strings(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let t = line.trim();
        if !t.contains("=> crate::models::SpaceDeviceKind::") {
            continue;
        }
        let Some(open) = t.find('"') else { continue };
        let Some(close_rel) = t[open + 1..].find('"') else {
            continue;
        };
        out.insert(t[open + 1..open + 1 + close_rel].to_string());
    }
    out
}

/// Every single-quoted value in `s`.
fn single_quoted(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\'' {
            let mut val = String::new();
            for c2 in it.by_ref() {
                if c2 == '\'' {
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
fn the_four_device_kind_vocabularies_agree() {
    let models = read(MODELS);
    let api = read(API);

    let sql = migration_labels();
    let to_sql = byte_strings_in_impl(&models, "ToSql<sql_types::SpaceDeviceKind");
    let from_sql = byte_strings_in_impl(&models, "FromSql<sql_types::SpaceDeviceKind");
    let accepted = api_accepted_strings(&api);

    assert!(
        sql.len() >= 2,
        "parsed too few enum labels from the migrations: {sql:?}"
    );

    assert_eq!(
        to_sql, sql,
        "ToSql writes labels Postgres does not have (or is missing one):\n  \
         to_sql: {to_sql:?}\n  sql:    {sql:?}"
    );
    assert_eq!(
        from_sql, sql,
        "FromSql cannot read every label Postgres can store:\n  \
         from_sql: {from_sql:?}\n  sql:      {sql:?}"
    );
    assert_eq!(
        accepted, sql,
        "the registration endpoint accepts a different set of kinds than exist:\n  \
         accepted: {accepted:?}\n  sql:      {sql:?}"
    );
}
