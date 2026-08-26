//! Five independent definitions of one wire format, and how they disagree.
//!
//! The server publishes a ToolGuard sync payload over MQTT. Five crates parse
//! it, and every one of them declares its own struct:
//!
//! | Where | Type |
//! |---|---|
//! | `server/src/api/toolguard.rs` | `ToolGuardSyncPayload` (the producer) |
//! | `edge/src/toolguard.rs` | `SyncPayload` |
//! | `kiosk/src/toolguard.rs` | `SyncPayload` |
//! | `toolguard-status-ui/src/main.rs` | `SyncPayload` |
//! | `toolguard-test-ui/src/mqtt_client.rs` | `SyncPayload` |
//!
//! They have already drifted, in both directions, and serde makes the drift
//! silent: an unknown field is ignored by default and a missing one is a parse
//! error at runtime, on a device, in a workshop.
//!
//! **This check does not assert they agree.** They do not, and pretending
//! otherwise by asserting a corrected version would mean editing four GUI
//! crates that are outside `default-members` because bevy and egui are heavy —
//! a change nobody in this repository can compile quickly, made from a test.
//!
//! What it does instead is state the divergence exactly, so that:
//!
//!   * a **sixth** copy fails immediately;
//!   * a change to any existing copy that widens the gap fails;
//!   * the producer growing a field that no consumer knows about fails.
//!
//! The fix is `css_lib::toolguard`, which has only serde dependencies and would
//! therefore cost the GUI crates nothing. That is written down in TESTING.md
//! §7 rather than done here, because it is a refactor across five crates and
//! this is a test.

use css_checks::{read, repo_root};
use std::collections::BTreeSet;

/// Field names of a struct, from its source.
fn fields_of(src: &str, name: &str) -> Option<BTreeSet<String>> {
    let at = src.find(&format!("struct {name} {{"))?;
    let body = &src[at..];
    let end = body.find("\n}")?;
    let body = &body[..end];

    let mut out = BTreeSet::new();
    for line in body.lines().skip(1) {
        let code = line.split("//").next().unwrap_or("").trim();
        let code = code.trim_start_matches("pub ").trim();
        if let Some(colon) = code.find(':') {
            let field = code[..colon].trim();
            if !field.is_empty() && field.chars().all(|c| c.is_alphanumeric() || c == '_') {
                out.insert(field.to_string());
            }
        }
    }
    Some(out)
}

struct Copy {
    where_: &'static str,
    file: &'static str,
    type_name: &'static str,
}

const COPIES: &[Copy] = &[
    Copy {
        where_: "server (the producer)",
        file: "server/src/api/toolguard.rs",
        type_name: "ToolGuardSyncPayload",
    },
    Copy {
        where_: "edge",
        file: "edge/src/toolguard.rs",
        type_name: "SyncPayload",
    },
    Copy {
        where_: "kiosk",
        file: "kiosk/src/toolguard.rs",
        type_name: "SyncPayload",
    },
    Copy {
        where_: "toolguard-status-ui",
        file: "toolguard-status-ui/src/main.rs",
        type_name: "SyncPayload",
    },
    Copy {
        where_: "toolguard-test-ui",
        file: "toolguard-test-ui/src/mqtt_client.rs",
        type_name: "SyncPayload",
    },
];

/// The divergence as it stands, asserted so it cannot widen unnoticed.
///
/// Read this as a description of a defect, not of a design. Every row that is
/// not the producer's full field set is a consumer that will fail to parse, or
/// silently ignore, part of what the server sends.
const EXPECTED_FIELDS: &[(&str, &[&str])] = &[
    (
        "server (the producer)",
        &["device_id", "profile_field", "tools", "users"],
    ),
    // The only consumer that matches the producer.
    ("edge", &["device_id", "profile_field", "tools", "users"]),
    // Drops device_id and profile_field. Harmless today because serde ignores
    // unknown fields -- and the reason it is harmless is a default, not a
    // decision.
    ("kiosk", &["tools", "users"]),
    ("toolguard-status-ui", &["tools", "users"]),
    // Drops `users` as well, so this one cannot show who is authorised for
    // anything.
    ("toolguard-test-ui", &["tools"]),
];

#[test]
fn every_copy_was_found() {
    // A path that stopped resolving would make its row vanish from the
    // comparison, and the check would then report agreement between four copies
    // while a fifth drifted freely.
    for c in COPIES {
        let src = read(c.file);
        assert!(
            fields_of(&src, c.type_name).is_some(),
            "{}: no `struct {}` in {}. If it was renamed, rename it here; if the \
             crate was deleted, delete its row -- but do not leave it silently \
             uncompared.",
            c.where_,
            c.type_name,
            c.file
        );
    }
}

#[test]
fn there_are_exactly_five_copies() {
    // A sixth is what this check is most worth catching. Every one of these was
    // written by somebody who needed to parse the payload and did not know the
    // other four existed.
    let mut found = Vec::new();
    let root = repo_root();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if matches!(name, "target" | "node_modules" | ".git") {
            continue;
        }
        // And not this crate. It quotes both type names in its own prose and
        // its own expectations, so counting itself would make the check report
        // a sixth copy that is the check.
        if dir.ends_with("checks") {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let src = std::fs::read_to_string(&path).unwrap_or_default();
                if src.contains("struct SyncPayload {")
                    || src.contains("struct ToolGuardSyncPayload {")
                {
                    found.push(
                        path.strip_prefix(&root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string(),
                    );
                }
            }
        }
    }
    found.sort();

    let mut expected: Vec<String> = COPIES.iter().map(|c| c.file.to_string()).collect();
    expected.sort();

    assert_eq!(
        found, expected,
        "the set of files declaring a ToolGuard sync payload has changed.\n\n\
         If this is a sixth copy: do not add it here. Move the type into \
         `css_lib::toolguard`, which has only serde dependencies and therefore \
         costs the GUI crates nothing. Five hand-maintained copies of one wire \
         format have already drifted in two directions; a sixth is not a \
         maintenance burden, it is a runtime parse failure waiting for a field \
         to be added."
    );
}

#[test]
fn the_divergence_is_exactly_what_is_recorded() {
    let mut wrong = Vec::new();

    for c in COPIES {
        let src = read(c.file);
        let Some(actual) = fields_of(&src, c.type_name) else {
            continue; // `every_copy_was_found` owns this
        };
        let expected: BTreeSet<String> = EXPECTED_FIELDS
            .iter()
            .find(|(w, _)| *w == c.where_)
            .map(|(_, f)| f.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        if actual != expected {
            let added: Vec<&String> = actual.difference(&expected).collect();
            let removed: Vec<&String> = expected.difference(&actual).collect();
            wrong.push(format!(
                "{} ({}): gained {:?}, lost {:?}",
                c.where_, c.file, added, removed
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "a ToolGuard payload definition changed:\n{}\n\n\
         Five crates parse this format and each declares its own struct, so a \
         field added to the producer and not to the consumers is ignored \
         silently by serde, and a field removed from the producer is a parse \
         error at runtime -- on a device, in a workshop, with no test anywhere \
         that would have seen it.\n\n\
         If you are fixing the divergence rather than widening it, the fix is \
         `css_lib::toolguard` and this whole file goes away with it.",
        wrong.join("\n")
    );
}

#[test]
fn no_consumer_knows_a_field_the_producer_does_not_send() {
    // The direction that is a bug today rather than a bug tomorrow: a consumer
    // declaring a field the server never sends fails to parse every message,
    // unless the field is Option -- and none of these are.
    let producer_src = read("server/src/api/toolguard.rs");
    let producer =
        fields_of(&producer_src, "ToolGuardSyncPayload").expect("the producer must be findable");

    for c in COPIES
        .iter()
        .filter(|c| c.where_ != "server (the producer)")
    {
        let src = read(c.file);
        let Some(consumer) = fields_of(&src, c.type_name) else {
            continue;
        };
        let unknown: Vec<&String> = consumer.difference(&producer).collect();
        assert!(
            unknown.is_empty(),
            "{} declares {:?}, which the server never sends. serde treats a \
             missing non-Option field as a parse error, so this consumer fails \
             on every message.",
            c.where_,
            unknown
        );
    }
}
