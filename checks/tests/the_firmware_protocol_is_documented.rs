//! `FIRMWARE.md` is the device wire contract, and it has to stay true.
//!
//! Firmware authors are the one audience that cannot read the source to settle
//! a question -- they are often on another team, in another language, on a
//! board with no Rust toolchain anywhere near it. A protocol document is the
//! only thing they have, which makes a *wrong* document worse than none: they
//! will trust it, and the failure surfaces as a tool that energizes when it
//! should not.
//!
//! Left to prose, it rots. There is a live example in this repository:
//! `toolguard-test-ui` was written against the protocol as it stood in early
//! September and nothing told it when `module-state`, `power-report` and the
//! interlock vocabulary arrived with #83 and #84. It is still MQTT-only. That
//! is what an unchecked description of a moving protocol becomes.
//!
//! So this check runs in **both directions**, and only one of them catches the
//! failure above:
//!
//! * Every `/api/...` path the document names must exist. Catches a **renamed
//!   or deleted** endpoint -- the document describing a server that is gone.
//! * Every device-facing route the server exposes must be named in the
//!   document. Catches a **new** endpoint landing undocumented, which is the
//!   `toolguard-test-ui` failure and the reason this file exists.
//! * Every `kind` in `css_lib::wire::kinds` must be documented, for the same
//!   reason in the MQTT half of the protocol.
//!
//! The route corpus is `e2e/corpus/endpoints.json`, which is generated from the
//! route table by `e2e/gen-endpoints.mjs` and regenerate-and-diffed by
//! `e2e/lint.sh`. It is the authoritative list precisely because it cannot be
//! hand-edited into agreement.
//!
//! Text-level, like its neighbours, so it needs no database and no compiler and
//! runs on the workstation where `css-server` cannot be built.
//!
//! What this does NOT prove: that the document is *accurate*. A payload field
//! described with the wrong type, or a status code that is simply wrong, passes
//! here. It proves the surface is completely and currently enumerated, which is
//! the part that rots silently; the prose around it still needs a reader.

use std::collections::BTreeSet;

use css_checks::read;

const DOC: &str = "FIRMWARE.md";
const CORPUS: &str = "e2e/corpus/endpoints.json";
const WIRE: &str = "css_lib/src/wire.rs";
/// The heading above the edge ↔ module tables in the document.
const LOCAL_SECTION: &str = "### The local broker: edge ↔ module";

/// Routes a device speaks, as opposed to routes *about* devices.
///
/// `/api/admin/devices/invite` is deliberately excluded: issuing an invite is an
/// administrator's job in a browser, not something firmware ever calls. Holding
/// the document to it would be demanding documentation of the wrong audience.
fn is_device_facing(template: &str) -> bool {
    template == "/api/toolguard"
        || template.starts_with("/api/toolguard/")
        || template == "/api/devices/register"
        || template == "/api/devices/ws"
}

/// `(method, template)` for every device-facing route the server exposes.
fn device_routes(corpus: &str) -> BTreeSet<(String, String)> {
    let parsed: serde_json::Value =
        serde_json::from_str(corpus).unwrap_or_else(|e| panic!("{CORPUS} is not valid JSON: {e}"));
    let endpoints = parsed["endpoints"]
        .as_array()
        .unwrap_or_else(|| panic!("{CORPUS} has no `endpoints` array"));

    let routes: BTreeSet<(String, String)> = endpoints
        .iter()
        .filter_map(|e| {
            let method = e["method"].as_str()?;
            let template = e["template"].as_str()?;
            is_device_facing(template).then(|| (method.to_string(), template.to_string()))
        })
        .collect();

    assert!(
        !routes.is_empty(),
        "{CORPUS} yielded no device-facing routes at all. Either the corpus \
         moved or `is_device_facing` no longer recognises anything -- and a \
         check with an empty corpus passes every assertion below vacuously."
    );
    routes
}

/// Does the document name this exact route?
///
/// Boundary-aware on purpose. `/api/toolguard` is a prefix of
/// `/api/toolguard/tool-on`, so a plain substring test would report the status
/// probe as documented whenever *any* toolguard endpoint was -- the one route
/// most likely to be forgotten would be the one that could never fail.
fn mentions(doc: &str, method: &str, template: &str) -> bool {
    let needle = format!("{method} {template}");
    let mut from = 0;
    while let Some(at) = doc[from..].find(&needle) {
        let end = from + at + needle.len();
        let next = doc[end..].chars().next();
        match next {
            Some(c) if c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' => {}
            _ => return true,
        }
        from = end;
    }
    false
}

/// Every `/api/...` path the document mentions, however it is written.
fn documented_paths(doc: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = doc.as_bytes();
    let mut i = 0;
    while let Some(at) = doc[i..].find("/api/") {
        let start = i + at;
        let mut end = start;
        while end < bytes.len() {
            let c = bytes[end] as char;
            if c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '{' | '}') {
                end += 1;
            } else {
                break;
            }
        }
        out.insert(doc[start..end].trim_end_matches('/').to_string());
        i = end.max(start + 1);
    }
    out
}

/// The `kind` string literals declared in `css_lib::wire::kinds`.
fn wire_kinds(wire: &str) -> BTreeSet<String> {
    let kinds: BTreeSet<String> = wire
        .lines()
        .filter(|l| l.trim_start().starts_with("pub const "))
        .filter_map(|l| {
            let open = l.find('"')?;
            let rest = &l[open + 1..];
            let close = rest.find('"')?;
            Some(rest[..close].to_string())
        })
        .collect();

    assert!(
        !kinds.is_empty(),
        "no `pub const` string literals found in {WIRE}. The extraction broke, \
         and an empty vocabulary makes the documentation check below vacuous."
    );
    kinds
}

/// Every backtick-delimited span in the document.
///
/// Used rather than a bare substring search because several kinds are ordinary
/// English words -- `data` and `name` would both "appear" in any prose long
/// enough, and a check satisfied by an accident of vocabulary is not a check.
fn code_spans(doc: &str) -> Vec<String> {
    doc.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.to_string())
        .collect()
}

/// A kind counts as documented when a code span is the kind itself, or is a
/// topic ending in it -- `heartbeat` is documented by
/// `{namespace}/devices/{device_id}/heartbeat`.
fn kind_is_documented(spans: &[String], kind: &str) -> bool {
    let suffix = format!("/{kind}");
    spans.iter().any(|s| s == kind || s.ends_with(&suffix))
}

/// The body of one `pub mod <name>` block in `wire.rs`.
///
/// `wire.rs` now holds two vocabularies -- `kinds` (server ↔ device, namespaced)
/// and `local` (edge ↔ module, not) -- and they are checked against different
/// parts of the document. Scanning the whole file for `pub const` would merge
/// them and report a missing local topic as a missing `kind`, sending whoever
/// reads the failure to the wrong table.
///
/// Delimited by the closing brace in column zero rather than by counting
/// braces: the constants' doc comments contain `{ card, tool_id }` payload
/// sketches, so brace counting reads the prose and loses its place.
fn module_body(src: &str, name: &str) -> String {
    let open = format!("pub mod {name} {{");
    let start = src
        .find(&open)
        .unwrap_or_else(|| panic!("no `{open}` in {WIRE}. The extraction broke."));
    let rest = &src[start + open.len()..];
    let end = rest
        .find("\n}")
        .unwrap_or_else(|| panic!("`pub mod {name}` in {WIRE} is never closed in column zero."));
    let body = rest[..end].to_string();
    assert!(
        body.contains("pub const"),
        "`pub mod {name}` in {WIRE} yielded no constants. An empty vocabulary \
         makes every check that reads it pass on any document at all."
    );
    body
}

/// Topic names the document's edge ↔ module tables name.
///
/// Scoped to that one section, and to backtick spans that look like a topic: a
/// slash, no whitespace, and not an HTTP path. The section's prose also
/// backticks `GET /api/toolguard/sync` and field names like `relay_on`, and
/// neither is a topic.
fn documented_local_topics(doc: &str) -> BTreeSet<String> {
    let start = match doc.find(LOCAL_SECTION) {
        Some(i) => i,
        None => return BTreeSet::new(),
    };
    let rest = &doc[start + LOCAL_SECTION.len()..];
    let section = match rest.find("\n### ") {
        Some(i) => &rest[..i],
        None => rest,
    };
    code_spans(section)
        .into_iter()
        .filter(|s| s.contains('/') && !s.contains(char::is_whitespace) && !s.starts_with("/api/"))
        .collect()
}

// ── The checks ───────────────────────────────────────────────────────────────

#[test]
fn every_device_facing_endpoint_is_documented() {
    let doc = read(DOC);
    let missing: Vec<String> = device_routes(&read(CORPUS))
        .into_iter()
        .filter(|(m, t)| !mentions(&doc, m, t))
        .map(|(m, t)| format!("  {m} {t}"))
        .collect();

    assert!(
        missing.is_empty(),
        "these device-facing routes exist on the server and are not in {DOC}:\n{}\n\n\
         Firmware authors cannot read the source to find them. An endpoint that \
         ships undocumented is one nobody outside this repository knows about, \
         which is how `toolguard-test-ui` ended up three features behind the \
         protocol it emulates.",
        missing.join("\n")
    );
}

#[test]
fn every_documented_endpoint_exists() {
    let corpus = read(CORPUS);
    let parsed: serde_json::Value = serde_json::from_str(&corpus).expect("corpus parses");
    let real: BTreeSet<String> = parsed["endpoints"]
        .as_array()
        .expect("endpoints array")
        .iter()
        .filter_map(|e| Some(e["template"].as_str()?.to_string()))
        .collect();

    let invented: Vec<String> = documented_paths(&read(DOC))
        .into_iter()
        .filter(|p| !real.contains(p))
        .collect();

    assert!(
        invented.is_empty(),
        "{DOC} documents paths the server does not serve: {invented:?}\n\n\
         A firmware author will implement against these and get a 404. Either \
         the route was renamed and the document needs to follow, or the \
         document was wrong when written."
    );
}

#[test]
fn every_wire_kind_is_documented() {
    let spans = code_spans(&read(DOC));
    let missing: Vec<String> = wire_kinds(&module_body(&read(WIRE), "kinds"))
        .into_iter()
        .filter(|k| !kind_is_documented(&spans, k))
        .collect();

    assert!(
        missing.is_empty(),
        "these `kind` strings are in {WIRE} and not in {DOC}: {missing:?}\n\n\
         Both transports route on them, so an undocumented kind is a message \
         firmware has no way to know it should handle."
    );
}

/// The edge ↔ module half, wire → document.
///
/// This is the direction that matters most for the local broker, because it is
/// the vocabulary firmware authors have never had. Until this section existed,
/// a module had no documented way to talk to its own coordinator -- the topics
/// lived as string literals in four separate crates and appeared in no document
/// at all.
#[test]
fn every_local_topic_is_documented() {
    let spans = code_spans(&read(DOC));
    let missing: Vec<String> = wire_kinds(&module_body(&read(WIRE), "local"))
        .into_iter()
        .filter(|t| !spans.iter().any(|s| s == t))
        .collect();

    assert!(
        missing.is_empty(),
        "these local-broker topics are in {WIRE} and not in {DOC}: {missing:?}\n\n\
         A module's counterparty is the edge, not the server. An undocumented \
         local topic is a message firmware cannot know to publish or subscribe \
         to, and no amount of reading the HTTP section will reveal it."
    );
}

/// And document → wire, which catches the opposite rot: a topic renamed in the
/// code while the table keeps describing the old one. Firmware written from the
/// stale table subscribes to a topic nothing ever publishes, and presents as a
/// device that connects, stays healthy, and silently does nothing.
#[test]
fn every_documented_local_topic_exists() {
    let real = wire_kinds(&module_body(&read(WIRE), "local"));
    let documented = documented_local_topics(&read(DOC));

    assert!(
        !documented.is_empty(),
        "no local-broker topics found under `{LOCAL_SECTION}` in {DOC}. Either \
         the section was renamed -- in which case this check has been passing \
         vacuously -- or the tables are gone."
    );

    let invented: Vec<String> = documented
        .into_iter()
        .filter(|t| !real.contains(t))
        .collect();

    assert!(
        invented.is_empty(),
        "{DOC} documents local-broker topics that are not in {WIRE}: {invented:?}\n\n\
         Firmware will subscribe to these and hear nothing, or publish to them \
         and be ignored -- a failure that looks like healthy silence from both \
         ends."
    );
}

// ── Self-tests ───────────────────────────────────────────────────────────────
//
// Each check above is fed the input it exists to reject. A check never observed
// failing is a check of unmeasured value -- and two of these have a plausible
// way to pass vacuously, which is exactly what the last two tests here pin
// down.

#[cfg(test)]
mod the_checks_reject_what_they_are_for {
    use super::*;

    const FAKE_CORPUS: &str = r#"{
        "endpoints": [
            { "method": "GET",  "template": "/api/toolguard/tool-on" },
            { "method": "POST", "template": "/api/toolguard/brand-new" },
            { "method": "GET",  "template": "/api/admin/devices/invite" }
        ]
    }"#;

    #[test]
    fn an_undocumented_endpoint_is_caught() {
        let doc = "### `GET /api/toolguard/tool-on`\n";
        let missing: Vec<_> = device_routes(FAKE_CORPUS)
            .into_iter()
            .filter(|(m, t)| !mentions(doc, m, t))
            .collect();
        assert_eq!(
            vec![("POST".to_string(), "/api/toolguard/brand-new".to_string())],
            missing,
            "a new endpoint absent from the document has to be reported, and an \
             admin-only route must not be"
        );
    }

    #[test]
    fn a_documented_endpoint_that_does_not_exist_is_caught() {
        let doc = "### `GET /api/toolguard/tool-on`\nand `/api/toolguard/removed-last-year`\n";
        let paths = documented_paths(doc);
        assert!(paths.contains("/api/toolguard/removed-last-year"));
        assert!(paths.contains("/api/toolguard/tool-on"));
    }

    #[test]
    fn an_undocumented_wire_kind_is_caught() {
        let wire = r#"pub const HEARTBEAT: &str = "heartbeat";
                      pub const MODULE_STATE: &str = "module/state";"#;
        let doc = "topic `{ns}/devices/{id}/heartbeat` carries a ping";
        let spans = code_spans(doc);
        let missing: Vec<_> = wire_kinds(wire)
            .into_iter()
            .filter(|k| !kind_is_documented(&spans, k))
            .collect();
        assert_eq!(vec!["module/state".to_string()], missing);
    }

    /// The prefix trap. `/api/toolguard` is a prefix of every other toolguard
    /// route, so a substring test would mark the status probe documented
    /// whenever any sibling was -- and the one endpoint most likely to be
    /// forgotten would be the one that could never fail.
    #[test]
    fn a_prefix_does_not_count_as_a_mention() {
        let doc = "### `GET /api/toolguard/tool-on`\n";
        assert!(mentions(doc, "GET", "/api/toolguard/tool-on"));
        assert!(
            !mentions(doc, "GET", "/api/toolguard"),
            "documenting a sub-path must not mark its parent documented"
        );
    }

    /// The vocabulary trap. `data` and `name` are ordinary English words; a
    /// bare substring search would find them in any prose of sufficient length
    /// and report the topic documented when it was never mentioned.
    #[test]
    fn prose_does_not_count_as_documenting_a_kind() {
        let doc = "The device sends data under whatever name you configured.";
        let spans = code_spans(doc);
        assert!(!kind_is_documented(&spans, "data"));
        assert!(!kind_is_documented(&spans, "name"));
    }

    /// And the corpus guard itself: a filter that matched nothing would make
    /// `every_device_facing_endpoint_is_documented` pass on any document at all.
    #[test]
    #[should_panic(expected = "no device-facing routes")]
    fn an_empty_corpus_is_refused_rather_than_passing_vacuously() {
        device_routes(r#"{ "endpoints": [ { "method": "GET", "template": "/api/users" } ] }"#);
    }

    /// The two vocabularies must not bleed into each other. Before the module
    /// scoping, one scan of the file merged them, so a local topic missing from
    /// the document was reported as a missing `kind` -- pointing whoever read
    /// the failure at the wrong table in the wrong section.
    #[test]
    fn the_two_vocabularies_are_extracted_separately() {
        let wire = read(WIRE);
        let kinds = wire_kinds(&module_body(&wire, "kinds"));
        let local = wire_kinds(&module_body(&wire, "local"));

        assert!(
            kinds.contains("module/state") && !local.contains("module/state"),
            "`module/state` is a server ↔ device kind and belongs only to `kinds`"
        );
        assert!(
            local.contains("toolguard/request/tool-on")
                && !kinds.contains("toolguard/request/tool-on"),
            "`toolguard/request/tool-on` is a local topic and belongs only to `local`"
        );
    }

    /// A module name that does not exist must fail loudly. Returning an empty
    /// body instead would make both local-topic checks pass on any document.
    #[test]
    #[should_panic(expected = "no `pub mod nonexistent")]
    fn a_missing_module_is_refused_rather_than_passing_vacuously() {
        module_body(&read(WIRE), "nonexistent");
    }

    /// The section-scoping guard. If the heading is ever reworded, the topic
    /// extractor finds nothing -- and a check that silently judges an empty set
    /// is one that has stopped running without saying so.
    #[test]
    fn a_renamed_section_yields_no_topics_rather_than_a_false_pass() {
        let doc = "### Some other heading\n\n| `toolguard/request/tool-on` | x |\n";
        assert!(
            documented_local_topics(doc).is_empty(),
            "topics outside the named section must not count -- the emptiness is \
             what `every_documented_local_topic_exists` asserts on"
        );
    }

    /// And the filter itself: the local section quotes HTTP paths and field
    /// names in backticks too, and neither is a topic.
    #[test]
    fn prose_and_http_paths_are_not_mistaken_for_local_topics() {
        let doc = format!(
            "{LOCAL_SECTION}\n\nthe twin of `GET /api/toolguard/sync`, and \
             `/api/toolguard/power-report`, where `relay_on` is absent.\n\n\
             | `toolguard/request/power` | x |\n"
        );
        let found = documented_local_topics(&doc);
        assert_eq!(
            found.into_iter().collect::<Vec<_>>(),
            vec!["toolguard/request/power".to_string()],
            "only the topic should survive the filter"
        );
    }
}
