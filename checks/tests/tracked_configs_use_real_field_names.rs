//! Every tracked config file names only fields `config.rs` actually declares.
//!
//! This exists because the same defect landed twice in one merge. dev renamed
//! `user.profile_fields` to `user.profile_fields_seed` and
//! `user.profiles_enabled` to `user.profiles_enabled_seed` -- the field schema
//! now lives in `profile_config_versions` and the config only seeds it on
//! first boot. `config.sample.toml` was renamed with it. `e2e/stack-config.toml`
//! was not.
//!
//! Neither failure is quiet at runtime, and that is precisely the problem:
//! neither field has a `#[serde(default)]`, so the old name is a missing-field
//! error, which `AppConfig::from_file` answers by backing up the file,
//! rewriting it in place, and refusing to boot. For the stack config that is a
//! bring-up that dies four stages deep with the cause in a container log.
//!
//! Both were caught, eventually, by `server/tests/stack_config_parses.rs` and
//! `config::tests::the_shipped_sample_config_parses` -- which are the right
//! tests and the authoritative ones, because they deserialize into the real
//! `AppConfig` and therefore also check types, not just names. But they live in
//! `css-server`, which does not compile on the FreeBSD workstation where the
//! merge happened: `dr-metrix-axum` calls `prometheus::process_collector`,
//! which prometheus gates behind `target_os = "linux"`. So each one cost a
//! container round trip to discover, one per run.
//!
//! This is the cheap half, moved to where the editing happens: names only, no
//! compiler, no types. It runs on any platform in milliseconds.
//!
//! What this does NOT prove: that a config *parses*. A field of the right name
//! and the wrong type passes here and fails there. This narrows the round-trip
//! cost of the common mistake; it does not replace the tests that settle it.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

/// Tracked files that are meant to deserialize as an `AppConfig`.
///
/// Listed rather than globbed: `Cargo.toml`, `rustfmt.toml` and
/// `.reaper.toml` are also tracked TOML and are not configs of this shape, and
/// a glob that had to exclude them would be one exclusion away from excluding
/// a real one. `every_listed_config_exists` keeps the list honest.
const CONFIGS: &[&str] = &["config.sample.toml", "e2e/stack-config.toml"];

/// Files that declare the structs an `AppConfig` deserializes into.
///
/// `css_lib` is here because `[edge.edge_mqtt_config]` deserializes into
/// `css_lib::MqttConfig` -- the edge's MQTT settings are shared with the edge
/// binary, so they live in the shared crate rather than in `config.rs`. A
/// version of this check that read only `config.rs` reported those three
/// fields as unknown, which would have been resolved by exempting three real
/// field names: the check would then have been permanently blind to a typo in
/// any of them.
const DECLARING_SOURCES: &[&str] = &["server/src/config.rs", "css_lib/src/lib.rs"];

/// Every field name declared in `DECLARING_SOURCES`, from the
/// `pub name: Type,` lines inside their structs.
///
/// Names, not paths: this cannot tell `user.profile_fields_seed` from
/// `pages.profile_fields_seed`, and does not try. A misplaced-but-real field
/// name is caught by the parse tests; a name that exists nowhere at all is the
/// mistake this file is for, and it is the one that keeps happening.
fn declared_fields() -> BTreeSet<String> {
    let source: String = DECLARING_SOURCES
        .iter()
        .map(|f| read(f))
        .collect::<Vec<_>>()
        .join("\n");
    let found: BTreeSet<String> = source
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub "))
        .filter_map(|l| l.split_once(':'))
        .map(|(name, _)| name.trim())
        // `pub fn`, `pub struct`, `pub enum` never contain a colon before the
        // name, but a `pub fn f(x: T)` line does. Field names are single
        // identifiers.
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(str::to_string)
        .collect();

    // Anti-vacuity: an empty or tiny set would make every assertion below pass
    // over nothing, which is the exact shape this whole crate exists to catch
    // elsewhere.
    assert!(
        found.len() >= 60,
        "parsed only {} field names out of {DECLARING_SOURCES:?}, which cannot \
         be right for files that size -- the declaration syntax changed and \
         this check is no longer reading it. Found: {found:?}",
        found.len()
    );
    assert!(
        found.contains("jwt_secret")
            && found.contains("profile_fields_seed")
            && found.contains("mqtt_instance_url"),
        "the parse is missing fields known to exist, so it is not reading the \
         structs correctly"
    );
    found
}

/// Every key named anywhere in a TOML document: table names, array-of-table
/// names, and the keys inside them, flattened.
fn keys_used(value: &toml::Value, out: &mut BTreeSet<String>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                out.insert(k.clone());
                keys_used(v, out);
            }
        }
        toml::Value::Array(a) => {
            for v in a {
                keys_used(v, out);
            }
        }
        _ => {}
    }
}

/// Keys that are legitimately not `AppConfig` field names, each with a reason.
///
/// Kept deliberately small. Every entry is a place this check cannot see the
/// truth, not a place the truth was inconvenient.
fn is_exempt(key: &str) -> bool {
    matches!(
        key,
        // Entries of `Vec<ProfileField>` / `Vec<ToolCategory>` / calendars:
        // these are element fields, declared in their own structs in the same
        // file, so they are found by `declared_fields` -- but `value`/`label`
        // are also generic enough to be worth naming here rather than relying
        // on that.
        "value" | "label"
    )
}

/// Replace `@NAME@` placeholders with `0`, so a template can be parsed.
///
/// `e2e/stack-config.toml` is deliberately not valid TOML before substitution:
/// `port` is an integer, so its placeholder cannot be quoted. `0` keeps an
/// integer field an integer and a quoted `"@FOO@"` a valid string.
///
/// The name pattern is `[A-Z0-9_]+` and that is load-bearing. A first draft
/// paired the next two `@` characters it found, which meant the `@` in
/// `from_email = "noreply@example.invalid"` paired with a later placeholder
/// and swallowed every line between them -- producing a duplicate-key error
/// that looked like a defect in the config rather than in this function.
fn substitute_placeholders(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == '@' {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_uppercase() || bytes[j].is_ascii_digit() || bytes[j] == '_')
            {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == '@' {
                out.push('0');
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

#[test]
fn the_placeholder_substitution_only_takes_placeholders() {
    // The bug above, pinned. Without this, the helper can regress into
    // swallowing text between an ordinary `@` and the next placeholder, and
    // every assertion in this file would then be running over a mangled
    // document.
    let s = substitute_placeholders("from = \"a@b.invalid\"\nport = @PG_PORT@\n");
    assert_eq!(s, "from = \"a@b.invalid\"\nport = 0\n");
}

#[test]
fn every_listed_config_exists() {
    for path in CONFIGS {
        assert!(
            repo_root().join(path).exists(),
            "{path} is listed in CONFIGS and does not exist. If it was deleted, \
             remove it from the list; if it was renamed, follow it -- a list \
             entry pointing at nothing silently drops a file from this check."
        );
    }
}

#[test]
fn no_tracked_config_names_a_field_that_does_not_exist() {
    let declared = declared_fields();
    let mut problems: Vec<String> = Vec::new();

    for path in CONFIGS {
        let text = read(path);
        // The stack config is deliberately not valid TOML before substitution
        // -- `port = @PORT@` is unquoted -- so placeholders are neutralized
        // first. Substituting a number keeps integer fields integers.
        let substituted = substitute_placeholders(&text);

        let parsed: toml::Value = match substituted.parse() {
            Ok(v) => v,
            Err(e) => {
                problems.push(format!("{path}: not valid TOML at all: {e}"));
                continue;
            }
        };

        let mut used = BTreeSet::new();
        keys_used(&parsed, &mut used);

        for key in used {
            if !declared.contains(&key) && !is_exempt(&key) {
                problems.push(format!("{path}: `{key}` is not a field in config.rs"));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "tracked configuration files name fields the config types do not \
         declare:\n\n{}\n\n\
         Neither `profiles_enabled_seed` nor `profile_fields_seed` carries a \
         serde default, so a stale name is a missing-field error --- and \
         `AppConfig::from_file` answers that by backing the file up, rewriting \
         it in place, and refusing to boot. For e2e/stack-config.toml that is \
         a bring-up that dies several stages later with the cause only in a \
         container log.\n\n\
         If a field was renamed in server/src/config.rs, rename it in every \
         file here too.",
        problems.join("\n")
    );
}

#[test]
fn the_two_configs_agree_on_the_user_section() {
    // The specific drift that happened, asserted directly rather than left to
    // fall out of the check above. These two files are edited by different
    // people for different reasons and nothing else compares them.
    let user_keys = |path: &str| -> BTreeSet<String> {
        let text = substitute_placeholders(&read(path));
        let parsed: toml::Value = text.parse().unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut out = BTreeSet::new();
        if let Some(user) = parsed.get("user") {
            if let Some(t) = user.as_table() {
                for k in t.keys() {
                    out.insert(k.clone());
                }
            }
        }
        out
    };

    let sample = user_keys("config.sample.toml");
    let stack = user_keys("e2e/stack-config.toml");

    assert!(
        !sample.is_empty() && !stack.is_empty(),
        "one of the configs has no [user] section, so this comparison proves \
         nothing: sample={sample:?} stack={stack:?}"
    );
    assert_eq!(
        sample, stack,
        "config.sample.toml and e2e/stack-config.toml disagree about what the \
         [user] section contains. One of them was updated for a rename and the \
         other was not."
    );
}
