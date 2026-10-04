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

// ── Accuracy, not just enumeration ───────────────────────────────────────────
//
// Everything above proves the *surface* is listed. The header of this file says
// plainly that it cannot prove the document is accurate -- "a payload field
// described with the wrong type, or a status code that is simply wrong, passes
// here" -- and an audit against the code in October 2026 found eight places
// where it had drifted, every one of them the kind a firmware author cannot
// discover without the source:
//
//   * `locked_tool_ids` was illustrated with an `external_id`; the server sends
//     UUIDs, so firmware matching its configured id found no lockout and
//     energized a locked-out tool. Fails open.
//   * tool `status` was illustrated as `Idle`; the enum serializes snake_case.
//   * the registration response had grown `command_key` -- an entire HMAC
//     command channel -- and the document never mentioned it, so firmware built
//     from it acts on unsigned unlock commands.
//   * `module/state` had no payload at all, so `params` (which GPIO to switch)
//     was undiscoverable.
//   * the `data` topic's required `uptime` and `platform` were unlisted, and a
//     payload missing them is dropped with only a server-side log.
//
// None of that is catchable by listing endpoints. What *is* mechanically
// checkable is that every name the wire carries appears in the document: field
// names, enumerated values, denial strings, and the byte layout of a signed
// message. These checks do that, code -> document, which is the direction that
// rots: a field is added to a struct and the prose is not told.
//
// They do not prove the prose around a name is *right*. Nothing can. They
// prove the name is there to be described, which is what makes a wrong
// description something a reader can notice.

/// Sources holding the vocabularies and payloads the wire carries.
const MODELS_MODULES: &str = "server/src/models/tool_modules.rs";
const MODELS_DEVICES: &str = "server/src/models/devices.rs";
const MODELS_TOOLS: &str = "server/src/models/tools.rs";
const TOOLGUARD: &str = "server/src/api/toolguard.rs";
const DEVICES_API: &str = "server/src/api/devices.rs";
const DEVICES_INBOUND: &str = "server/src/devices_inbound.rs";
const DOORS: &str = "server/src/doors.rs";
const SIG: &str = "css_lib/src/sig.rs";

/// Is this field name written down as a field, rather than merely occurring?
///
/// Either a backtick span that *is* the name, or a JSON key `"name"`. A bare
/// substring test would be satisfied by prose for every short field --
/// `id`, `name`, `role`, `status`, `grant` -- and report the most easily
/// forgotten fields as the ones that can never fail.
fn field_is_documented(doc: &str, spans: &[String], field: &str) -> bool {
    spans.iter().any(|s| s == field) || doc.contains(&format!("\"{field}\""))
}

/// Field names of one struct, `pub` or private, by text.
///
/// Private too, because two payloads this document is answerable for are local
/// structs inside their handlers -- `DeviceDataPayload` and `DoorEventIn` --
/// and those are precisely the two whose required fields were missing.
fn struct_fields(src: &str, name: &str) -> Vec<String> {
    // The declaration line, and the indentation it sits at. A struct declared
    // inside a handler closes at `        }`, not in column zero -- and scanning
    // past it swallows the rest of the function, which reads as a dozen phantom
    // fields from whatever locals follow.
    let (decl_line, indent) = src
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start();
            let matches = trimmed == format!("pub struct {name} {{")
                || trimmed == format!("struct {name} {{");
            matches.then(|| {
                (
                    line.to_string(),
                    line[..line.len() - trimmed.len()].to_string(),
                )
            })
        })
        .unwrap_or_else(|| {
            panic!(
                "no `struct {name}` declaration found. It was renamed or moved, \
                 and this check silently stopped covering it"
            )
        });

    let mut fields = Vec::new();
    let mut inside = false;
    let closing = format!("{indent}}}");
    for line in src.lines() {
        if !inside {
            inside = line == decl_line;
            continue;
        }
        if line == closing {
            break;
        }
        let trimmed = line.trim();
        let decl = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
        if decl.starts_with("//") || decl.starts_with('#') {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let ident = decl[..colon].trim();
            if !ident.is_empty()
                && ident
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                fields.push(ident.to_string());
            }
        }
    }
    assert!(
        !fields.is_empty(),
        "`struct {name}` yielded no fields; the extraction broke and this check \
         is now passing vacuously"
    );
    fields
}

/// Serialized variant names of a `#[serde(rename_all = "snake_case")]` enum.
fn snake_case_enum_values(src: &str, name: &str) -> Vec<String> {
    let at = src
        .find(&format!("pub enum {name} {{"))
        .unwrap_or_else(|| panic!("no `pub enum {name}` found in the expected file"));
    // Anti-vacuity, and the reason this check exists at all: the conversion
    // below is only correct while the enum really is renamed snake_case. If the
    // attribute goes, the documented values change and this must fail rather
    // than keep asserting the old spelling.
    let before = &src[..at];
    assert!(
        before.contains(r#"#[serde(rename_all = "snake_case")]"#),
        "`{name}` is no longer `#[serde(rename_all = \"snake_case\")]`. Its \
         serialized values have changed, so {DOC} and this check both need \
         revisiting -- do not simply delete the assertion."
    );

    let rest = &src[at..];
    let body = &rest[..rest.find("\n}").expect("enum is closed")];
    let mut out = Vec::new();
    for line in body.lines().skip(1) {
        let line = line.trim().trim_end_matches(',');
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if !line.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        let mut snake = String::new();
        for (i, c) in line.chars().enumerate() {
            if c.is_ascii_uppercase() {
                if i != 0 {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            } else {
                snake.push(c);
            }
        }
        out.push(snake);
    }
    assert!(!out.is_empty(), "`pub enum {name}` yielded no variants");
    out
}

/// Literal denial and acknowledgement strings a device can receive.
///
/// Only literals: the billing path answers with a computed reason, which the
/// document covers as "(billing-specific text)" because there is nothing
/// stable to enumerate.
fn literal_device_messages(src: &str) -> BTreeSet<String> {
    // Whitespace-tolerant between the paren and the literal, because rustfmt
    // wraps the long ones onto their own line:
    //
    //     return Ok(Json(ToolGuardResponse::tool_denied(
    //         "Metered tool requires a power-bound device",
    //     )));
    //
    // The first version of this matched `tool_denied("` on one line and
    // therefore skipped every wrapped message -- including the one message this
    // whole check was written for. It passed, and it covered nothing.
    let mut out = BTreeSet::new();
    for opener in [
        "tool_denied(",
        "ToolGuardResponse::error(",
        "ok_with_message(",
    ] {
        let mut from = 0;
        while let Some(at) = src[from..].find(opener) {
            let after = from + at + opener.len();
            let rest = &src[after..];
            let trimmed = rest.trim_start();
            let skipped = rest.len() - trimmed.len();
            from = after;
            // A dynamic argument (`&reason`) has no literal to enumerate; the
            // document covers that case as "(billing-specific text)".
            if !trimmed.starts_with('"') {
                continue;
            }
            let body = &rest[skipped + 1..];
            if let Some(end) = body.find('"') {
                out.insert(body[..end].to_string());
                from = after + skipped + 1 + end;
            }
        }
    }
    assert!(
        out.len() >= 8,
        "only {} literal device messages found in {TOOLGUARD}, which is too few \
         to be right -- the extraction broke and this check is covering a \
         subset of the messages it claims to: {out:?}",
        out.len()
    );
    out
}

#[test]
fn every_enumerated_value_is_documented() {
    let doc = read(DOC);
    let spans = code_spans(&doc);

    let mut expected: Vec<(String, String)> = Vec::new();
    let modules = read(MODELS_MODULES);
    for vocab in [
        "binding_role",
        "on_disconnect",
        "interlock_kind",
        "interlock_condition",
        "interlock_reset",
        "enforcement",
    ] {
        for value in wire_kinds(&module_body(&modules, vocab)) {
            expected.push((vocab.to_string(), value));
        }
    }
    for value in wire_kinds(&module_body(&read(MODELS_DEVICES), "device_role")) {
        expected.push(("device_role".to_string(), value));
    }
    for value in snake_case_enum_values(&read(MODELS_TOOLS), "ToolStatus") {
        expected.push(("ToolStatus".to_string(), value));
    }

    assert!(
        expected.len() > 25,
        "only {} enumerated values extracted, which is too few to be right -- \
         the vocabularies moved and this check is passing vacuously",
        expected.len()
    );

    let missing: Vec<String> = expected
        .into_iter()
        .filter(|(_, v)| !spans.iter().any(|s| s == v))
        .map(|(vocab, v)| format!("  {vocab}::{v}"))
        .collect();

    assert!(
        missing.is_empty(),
        "these values the server accepts or sends are not written down in \
         {DOC}:\n{}\n\n\
         Firmware has to branch on them. An undocumented interlock condition \
         is a rule nobody can implement -- and the document's own instruction \
         is to treat an unrecognised condition as a hazard, so the tool stays \
         off and nobody knows why.",
        missing.join("\n")
    );
}

#[test]
fn every_wire_payload_field_is_documented() {
    let doc = read(DOC);
    let spans = code_spans(&doc);

    let sources: Vec<(&str, String)> = vec![
        (WIRE, read(WIRE)),
        (TOOLGUARD, read(TOOLGUARD)),
        (DEVICES_API, read(DEVICES_API)),
        (DEVICES_INBOUND, read(DEVICES_INBOUND)),
        (DOORS, read(DOORS)),
    ];
    let src = |file: &str| -> String {
        sources
            .iter()
            .find(|(f, _)| *f == file)
            .map(|(_, s)| s.clone())
            .expect("source listed")
    };

    // Payloads a device sends or receives. Each named deliberately: a struct
    // added to the wire has to be added here, and a struct renamed fails in
    // `struct_fields` rather than quietly losing its coverage.
    let payloads: [(&str, &str); 19] = [
        (WIRE, "ToolModuleStatePayload"),
        (WIRE, "ToolModuleTool"),
        (WIRE, "DeviceBinding"),
        (WIRE, "ToolInterlockRule"),
        (WIRE, "PowerStatePayload"),
        (WIRE, "PowerStateCircuit"),
        (WIRE, "PowerStateTool"),
        (WIRE, "ToolLeasePayload"),
        (TOOLGUARD, "ToolGuardResponse"),
        (TOOLGUARD, "ToolGuardSyncPayload"),
        (TOOLGUARD, "ToolGuardSyncTool"),
        (TOOLGUARD, "ToolGuardSyncUser"),
        (TOOLGUARD, "PowerReportRequest"),
        (TOOLGUARD, "ToolLogRequest"),
        (TOOLGUARD, "PowerTripRequest"),
        (DEVICES_API, "RegisterDeviceRequest"),
        (DEVICES_API, "RegisterDeviceResponse"),
        (DEVICES_API, "EdgeMqttConfig"),
        (DEVICES_INBOUND, "DeviceDataPayload"),
    ];

    let mut missing: Vec<String> = Vec::new();
    for (file, name) in payloads {
        for field in struct_fields(&src(file), name) {
            if !field_is_documented(&doc, &spans, &field) {
                missing.push(format!("  {name}.{field}  ({file})"));
            }
        }
    }
    // The two door payloads, whose structs live beside each other.
    let doors_src = src(DOORS);
    for name in ["DoorStateSnapshot", "CompiledDoor"] {
        for field in struct_fields(&doors_src, name) {
            if !field_is_documented(&doc, &spans, &field) {
                missing.push(format!("  {name}.{field}  ({DOORS})"));
            }
        }
    }
    let inbound = src(DEVICES_INBOUND);
    for field in struct_fields(&inbound, "DoorEventIn") {
        if !field_is_documented(&doc, &spans, &field) {
            missing.push(format!("  DoorEventIn.{field}  ({DEVICES_INBOUND})"));
        }
    }

    assert!(
        missing.is_empty(),
        "these fields cross the wire to or from a device and are not named in \
         {DOC}:\n{}\n\n\
         A field a firmware author cannot see is one they cannot send -- and \
         for a *required* field, the server drops the whole message with only a \
         log line on its own side. `command_key` was absent for months this \
         way, and firmware written without it acts on unsigned commands.",
        missing.join("\n")
    );
}

#[test]
fn every_literal_denial_message_is_documented() {
    let doc = read(DOC);
    let missing: Vec<String> = literal_device_messages(&read(TOOLGUARD))
        .into_iter()
        .filter(|m| !doc.contains(m))
        .collect();

    assert!(
        missing.is_empty(),
        "these messages a device can receive are not in {DOC}: {missing:?}\n\n\
         The denial table is the only place a firmware author can learn what a \
         refusal means. A message changed in the code and not here sends them \
         looking for the wrong cause -- which is exactly what `Metered tool \
         requires its own API key` did after #101 retired API keys."
    );
}

#[test]
fn no_device_message_names_a_retired_credential() {
    // The specific regression, pinned. #101 removed `external_api_key` and
    // `toolguard.global_api_key`; the denial text kept naming them for a year,
    // so the one message a firmware author was most likely to hit told them to
    // go and find a credential that does not exist.
    let offenders: Vec<String> = literal_device_messages(&read(TOOLGUARD))
        .into_iter()
        .filter(|m| {
            let lower = m.to_lowercase();
            lower.contains("api key") || lower.contains("api_key")
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these device-facing messages name a credential this server does not \
         accept: {offenders:?}\n\n\
         There is one credential, a device token, and what gates a metered tool \
         is a `power`-role binding. Say that instead."
    );
}

#[test]
fn the_signed_message_layouts_match_the_document() {
    let sig = read(SIG);
    let doc = read(DOC);

    // The canonical strings are the format literals in `css_lib::sig`. If the
    // layout changes, every firmware's signatures stop verifying at once, and
    // the only way an author learns the new one is from this document.
    let mut found = 0;
    for line in sig.lines() {
        let trimmed = line.trim();
        let Some(open) = trimmed.find("format!(\"") else {
            continue;
        };
        let rest = &trimmed[open + "format!(\"".len()..];
        let Some(close) = rest.find('"') else {
            continue;
        };
        let layout = &rest[..close];
        if !layout.contains('|') {
            continue;
        }
        found += 1;
        assert!(
            doc.contains(layout),
            "the signed-message layout `{layout}` from {SIG} does not appear in \
             {DOC}.\n\n\
             A device computes its MAC over exactly these bytes. A layout the \
             document gets wrong produces signatures that are stable, \
             plausible and rejected on every message."
        );
    }
    assert!(
        found >= 2,
        "found {found} canonical signing layouts in {SIG}, expected at least \
         two (unlock and event). The extraction broke, and this check now \
         passes on any document at all."
    );
}

// ── Self-tests for the accuracy checks ───────────────────────────────────────
//
// Same discipline as above: each extractor is fed the input it exists to
// reject. The first of these is not hypothetical -- it is the bug this file's
// own author wrote and the suite caught, which is the whole argument for
// writing them.

#[cfg(test)]
mod the_accuracy_checks_reject_what_they_are_for {
    use super::*;

    /// A struct declared *inside a function* closes at its own indentation. The
    /// first version of `struct_fields` scanned to the first `\n}` in column
    /// zero, swallowed the rest of the handler, and reported a dozen phantom
    /// fields from the locals that followed -- which would have demanded the
    /// document describe `event_data` as part of a door event.
    #[test]
    fn a_struct_inside_a_function_stops_at_its_own_brace() {
        let src = "\
pub async fn handle(&self) {
        #[derive(Deserialize)]
        struct Inner {
            door_id: Uuid,
            granted: bool,
        }

        let audit = NewAuditLog {
            event_type: something,
            user_agent: None,
        };
    }
";
        assert_eq!(
            vec!["door_id".to_string(), "granted".to_string()],
            struct_fields(src, "Inner"),
            "only the struct's own fields may be reported"
        );
    }

    #[test]
    fn a_top_level_struct_still_works() {
        let src = "pub struct Outer {\n    pub a: String,\n    pub b: i32,\n}\n";
        assert_eq!(
            vec!["a".to_string(), "b".to_string()],
            struct_fields(src, "Outer")
        );
    }

    #[test]
    #[should_panic(expected = "no `struct Gone` declaration found")]
    fn a_renamed_struct_is_refused_rather_than_silently_uncovered() {
        struct_fields("pub struct Other { pub a: String }", "Gone");
    }

    /// The vocabulary trap again, for fields. `id`, `name`, `role` and `status`
    /// occur in any protocol prose; a substring test would mark the most
    /// forgettable fields as the ones that can never fail.
    #[test]
    fn prose_does_not_count_as_documenting_a_field() {
        let doc = "The device sends its id and name, and the role it plays.";
        let spans = code_spans(doc);
        for field in ["id", "name", "role"] {
            assert!(
                !field_is_documented(doc, &spans, field),
                "`{field}` in prose must not count as documented"
            );
        }
    }

    #[test]
    fn a_json_key_or_a_code_span_counts() {
        let doc = "the payload carries `params`, and \"uptime\": 86400 in seconds";
        let spans = code_spans(doc);
        assert!(field_is_documented(doc, &spans, "params"));
        assert!(field_is_documented(doc, &spans, "uptime"));
        assert!(!field_is_documented(doc, &spans, "command_key"));
    }

    #[test]
    fn enum_variants_become_their_serialized_spelling() {
        let src = "\
#[derive(Serialize)]
#[serde(rename_all = \"snake_case\")]
pub enum ToolStatus {
    Idle,
    InUse,
    Maintenance,
}
";
        assert_eq!(
            vec![
                "idle".to_string(),
                "in_use".to_string(),
                "maintenance".to_string()
            ],
            snake_case_enum_values(src, "ToolStatus"),
            "`InUse` serializes as `in_use`, which is what the document must say"
        );
    }

    /// The conversion is only correct while the attribute is there. Losing it
    /// changes every value on the wire, so the check must fail rather than go
    /// on asserting the old spelling.
    #[test]
    #[should_panic(expected = "no longer")]
    fn losing_the_rename_attribute_is_refused() {
        let src = "pub enum ToolStatus {\n    Idle,\n}\n";
        snake_case_enum_values(src, "ToolStatus");
    }

    /// Both layouts, because rustfmt chooses between them by line length and
    /// the wrapped one is where the long messages live. An extractor that only
    /// reads the inline form passes while covering none of them.
    #[test]
    fn literal_messages_are_extracted_in_either_layout() {
        let src = r#"
            return Ok(Json(ToolGuardResponse::tool_denied("Training required")));
            return Ok(Json(ToolGuardResponse::tool_denied(&reason)));
            return Ok(Json(ToolGuardResponse::tool_denied(
                "Metered tool requires a power-bound device",
            )));
            return Ok(Json(ToolGuardResponse::error("Tool not found")));
            ToolGuardResponse::ok_with_message(
                "Usage logged",
            )
            ToolGuardResponse::tool_denied("Unknown card")
            ToolGuardResponse::tool_denied("User is not active")
            ToolGuardResponse::tool_denied("Tool is broken")
            ToolGuardResponse::tool_denied("Tool is retired")
        "#;
        let found = literal_device_messages(src);
        for expected in [
            "Training required",
            "Metered tool requires a power-bound device",
            "Tool not found",
            "Usage logged",
        ] {
            assert!(found.contains(expected), "missing {expected:?}: {found:?}");
        }
        assert!(
            !found.iter().any(|m| m.contains("reason")),
            "the computed billing reason has no literal to enumerate and must \
             not become one: {found:?}"
        );
    }

    #[test]
    fn a_broken_internal_link_is_caught() {
        // And the arrow case, which is the one a hand-written anchor gets
        // wrong: the vanished `↔` leaves a double hyphen behind.
        assert_eq!(
            "the-local-broker-edge--module",
            github_anchor("The local broker: edge ↔ module")
        );
        assert_eq!(
            "enforcement-firmware",
            github_anchor("`enforcement: firmware`")
        );
        assert_eq!("on_disconnect", github_anchor("`on_disconnect`"));
        // Capitals fold and a trailing space is trimmed rather than becoming a
        // hyphen, which is also what GitHub does -- so a heading edited to add
        // a trailing space does not break every link to it.
        assert_eq!("device-classes", github_anchor("Device Classes "));
        // And a link to something that is not a heading is caught, which is
        // the whole point.
        let anchors: BTreeSet<String> = ["device-classes".to_string()].into_iter().collect();
        assert!(!anchors.contains(&github_anchor("Device Clases")));
    }

    #[test]
    fn a_retired_credential_in_a_message_is_caught() {
        let src = r#"
            tool_denied("Metered tool requires its own API key")
            tool_denied("a") tool_denied("b") tool_denied("c") tool_denied("d")
            tool_denied("e") tool_denied("f") tool_denied("g") tool_denied("h")
        "#;
        let offenders: Vec<String> = literal_device_messages(src)
            .into_iter()
            .filter(|m| {
                let lower = m.to_lowercase();
                lower.contains("api key") || lower.contains("api_key")
            })
            .collect();
        assert_eq!(
            1,
            offenders.len(),
            "the message that outlived #101 must be caught"
        );
    }
}

/// GitHub's heading-anchor algorithm: lower-case, drop everything that is not
/// alphanumeric, space, hyphen or underscore, then spaces become hyphens.
///
/// Reproduced rather than approximated because the document's own headings
/// contain backticks, colons and an `↔`, and each is dropped differently --
/// `### The local broker: edge ↔ module` becomes
/// `the-local-broker-edge--module`, with the double hyphen the vanished arrow
/// leaves behind.
fn github_anchor(heading: &str) -> String {
    let cleaned: String = heading
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect();
    cleaned.trim().replace(' ', "-")
}

#[test]
fn every_internal_link_resolves() {
    let doc = read(DOC);

    let anchors: BTreeSet<String> = doc
        .lines()
        .filter_map(|l| l.strip_prefix('#'))
        .map(|rest| github_anchor(rest.trim_start_matches('#').trim()))
        .collect();
    assert!(
        anchors.len() > 20,
        "only {} headings found in {DOC}; the extraction broke",
        anchors.len()
    );

    let mut broken = Vec::new();
    let mut from = 0;
    while let Some(at) = doc[from..].find("](#") {
        let start = from + at + "](#".len();
        let end = match doc[start..].find(')') {
            Some(e) => start + e,
            None => break,
        };
        let target = &doc[start..end];
        if !anchors.contains(target) {
            broken.push(target.to_string());
        }
        from = end;
    }

    assert!(
        broken.is_empty(),
        "{DOC} links to anchors it does not contain: {broken:?}\n\n\
         The document is now long enough to be navigated rather than read \
         start to finish, and the walkthrough leans on those links to keep the \
         reference material out of the way. A link that goes nowhere sends a \
         firmware author back to guessing."
    );
}

// ── The local broker's response payloads, row by row ─────────────────────────
//
// `every_wire_payload_field_is_documented` above walks *structs*. The edge's
// replies to a module are not structs: three of the five are
// `serde_json::json!` literals built inside their handlers, so that check
// cannot see them and never could. The gap was not hypothetical -- the document
// described all three `toolguard/response/*` topics as carrying the server's
// `ToolGuardResponse`, and said `tool_on: true` was "the *only* thing that
// means energize", when the local wire carries `authorized` and has no
// `tool_on` field anywhere. Firmware written to that sentence looks for a field
// that never arrives, finds nothing, and never starts a tool.
//
// WHY THIS IS ROW-SCOPED, which the checks above are not. They ask whether a
// name appears *somewhere* in the document, which is the right question for a
// vocabulary and the wrong one for a payload. Both defects this check exists
// for are invisible to a whole-document search: `tool_on` is legitimately
// documented for `POST /api/toolguard/tool-on`, and `granted` is legitimately
// documented for `doors/event`. Each was present in the document and absent
// from the row that had to carry it, so only a per-topic comparison can fail.
const EDGE_MQTT: &str = "edge/src/mqtt.rs";
const EDGE_DOORS: &str = "edge/src/doors.rs";

/// Where one topic's payload is actually defined.
enum Payload {
    /// The keys of the `json!` literals this handler publishes.
    Handler(&'static str),
    /// The serde field names of a struct, `(file, name)`.
    Struct(&'static str, &'static str),
}

/// Topic -> its source of truth, duplicated here deliberately.
///
/// This mapping is the check. Deriving it from the code would make the test
/// agree with whatever the code does, which is the one thing a documentation
/// oracle must not do: the question is whether the *document* matches, and that
/// needs an independent statement of which source answers for which row.
const LOCAL_RESPONSE_PAYLOADS: &[(&str, Payload)] = &[
    (
        "toolguard/response/tool-on",
        Payload::Handler("handle_tool_on"),
    ),
    (
        "toolguard/response/tool-off",
        Payload::Handler("handle_tool_off"),
    ),
    (
        "toolguard/response/tool-log",
        Payload::Handler("handle_tool_log"),
    ),
    ("toolguard/response/power", Payload::Handler("handle_power")),
    (
        "door/response/unlock",
        Payload::Struct(EDGE_DOORS, "LocalUnlockResponse"),
    ),
];

/// The body of one function, by text, from its declaration to the `}` at its
/// own indentation. Same reasoning as `struct_fields`: closing on the first
/// `}` in column zero swallows every later function in the `impl`.
fn fn_body(src: &str, name: &str) -> String {
    let (start, indent) = src
        .lines()
        .enumerate()
        .find_map(|(i, line)| {
            let trimmed = line.trim_start();
            let hit = trimmed.starts_with(&format!("fn {name}("))
                || trimmed.starts_with(&format!("async fn {name}("))
                || trimmed.starts_with(&format!("pub fn {name}("))
                || trimmed.starts_with(&format!("pub async fn {name}("));
            hit.then(|| (i, line.len() - trimmed.len()))
        })
        .unwrap_or_else(|| {
            panic!("no `fn {name}` found; the mapping names a function that is gone")
        });

    let close = format!("{}}}", " ".repeat(indent));
    let mut out = Vec::new();
    for line in src.lines().skip(start) {
        let last = line == close && !out.is_empty();
        out.push(line);
        if last {
            break;
        }
    }
    out.join("\n")
}

/// Index just past the `)` matching the `(` at `open`.
fn balanced_paren(src: &str, open: usize) -> usize {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    src.len()
}

/// Every `"name"` used as a JSON key in a span.
fn json_keys(span: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut from = 0;
    while let Some(at) = span[from..].find('"') {
        let start = from + at + 1;
        let Some(len) = span[start..].find('"') else {
            break;
        };
        let name = &span[start..start + len];
        let after = span[start + len + 1..].trim_start();
        if after.starts_with(':') && !name.is_empty() {
            out.insert(name.to_string());
        }
        from = start + len + 1;
    }
    out
}

/// The keys of the `json!` literals a handler actually *publishes*.
///
/// Scoped to literals that are either bound to `response_payload` or passed
/// straight to `publish_local`, because a handler builds other JSON too:
/// `handle_tool_on` also constructs the HTTP forward body `{card, tool_id}`,
/// and collecting the whole function would report those as part of the reply.
///
/// That scoping is a naming convention, and it fails in the safe direction: if
/// the binding is renamed the key set goes empty and the anti-vacuity assertion
/// below fails loudly, which tells the next reader to update this mapping
/// rather than silently checking nothing.
fn published_json_keys(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut from = 0;
    while let Some(at) = body[from..].find("json!(") {
        let start = from + at;
        let stmt_start = body[..start]
            .rfind(|c: char| c == ';' || c == '{')
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = &body[stmt_start..start];
        let published =
            prefix.contains("publish_local(") || prefix.contains("let response_payload");
        let end = balanced_paren(body, start + "json!".len());
        if published {
            out.extend(json_keys(&body[start..end]));
        }
        from = end;
    }
    out
}

/// Every quoted identifier in a span, whether or not a `:` follows it.
///
/// Separate from [`json_keys`] on purpose, because the two sides are written
/// differently and each needs its own reader. Source is Rust, where a key is
/// always `"name":` and the colon is what distinguishes a key from a string
/// *value*. The document's tables use two styles -- `{ "card", "tool_id" }` for
/// a bare field list and `{ "authorized": bool }` when the type matters -- and
/// demanding colons there would read the bare rows as carrying no fields at
/// all, which is a false pass rather than a false failure.
///
/// Restricted to identifier shape (lowercase, digits, `_`) so that a quoted
/// word in the surrounding prose is not read as a field. It would still read a
/// quoted *value* like `"edge"` as one; no row in the table has that, and a row
/// that grows one will fail loudly here rather than quietly.
fn quoted_names(span: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut from = 0;
    while let Some(at) = span[from..].find('"') {
        let start = from + at + 1;
        let Some(len) = span[start..].find('"') else {
            break;
        };
        let name = &span[start..start + len];
        let identifier_shaped = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if identifier_shaped {
            out.insert(name.to_string());
        }
        from = start + len + 1;
    }
    out
}

/// The field names a topic's row in the document claims it carries.
fn documented_payload_fields(doc: &str, topic: &str) -> BTreeSet<String> {
    let section = match doc.split(LOCAL_SECTION).nth(1) {
        Some(s) => s,
        None => return BTreeSet::new(),
    };
    let needle = format!("`{topic}`");
    section
        .lines()
        .find(|l| l.starts_with('|') && l.contains(&needle))
        .and_then(|row| row.split('|').nth(2).map(quoted_names))
        .unwrap_or_default()
}

/// Every field the edge publishes on a local response topic is named in that
/// topic's own row, and every field the row names is one the edge publishes.
#[test]
fn every_local_response_payload_matches_its_row() {
    let doc = read(DOC);
    let mqtt = read(EDGE_MQTT);

    for (topic, payload) in LOCAL_RESPONSE_PAYLOADS {
        let actual: BTreeSet<String> = match payload {
            Payload::Handler(name) => published_json_keys(&fn_body(&mqtt, name)),
            Payload::Struct(file, name) => struct_fields(&read(file), name).into_iter().collect(),
        };
        let documented = documented_payload_fields(&doc, topic);

        assert!(
            !actual.is_empty(),
            "extracted no payload fields at all for `{topic}` from the source. \
             The handler or struct named in LOCAL_RESPONSE_PAYLOADS has moved or \
             been renamed, and this row has been checking nothing."
        );
        assert!(
            !documented.is_empty(),
            "{DOC} has no row naming any payload field for `{topic}` under \
             `{LOCAL_SECTION}`. Either the table moved -- in which case this \
             check was passing vacuously -- or the row lost its payload cell."
        );

        let undocumented: Vec<&String> = actual.difference(&documented).collect();
        assert!(
            undocumented.is_empty(),
            "the edge publishes these fields on `{topic}` and {DOC}'s row for it \
             does not name them: {undocumented:?}\n\n\
             A firmware author reads one row to learn one message. A field that \
             is described elsewhere in the document, or nowhere, is a field they \
             will not know to read -- and on these topics the omitted field has \
             twice been the one that decides whether to energize."
        );

        let invented: Vec<&String> = documented.difference(&actual).collect();
        assert!(
            invented.is_empty(),
            "{DOC}'s row for `{topic}` names fields the edge never publishes: \
             {invented:?}\n\n\
             Firmware written from this row looks for a field that never \
             arrives. Absence is not permission, so it fails closed -- as a \
             tool that never starts, with no error at either end. This is the \
             defect the check was added for: the row said `tool_on`, the wire \
             carries `authorized`."
        );
    }
}

#[cfg(test)]
mod the_payload_row_check_rejects_what_it_is_for {
    use super::*;

    /// The real table with `authorized` swapped back to the old wrong claim.
    #[test]
    fn a_row_naming_a_field_the_wire_does_not_carry_fails() {
        let doc = read(DOC).replace(
            r#"| `toolguard/response/tool-on` | `{ "authorized": bool, "reason": string }` |"#,
            r#"| `toolguard/response/tool-on` | `{ "status": string, "tool_on": bool }` |"#,
        );
        let documented = documented_payload_fields(&doc, "toolguard/response/tool-on");
        let actual = published_json_keys(&fn_body(&read(EDGE_MQTT), "handle_tool_on"));

        assert!(
            documented.contains("tool_on"),
            "the mutant did not apply; the row's text has changed and this \
             self-test is no longer driving the arm it names"
        );
        assert!(
            !documented
                .difference(&actual)
                .collect::<Vec<_>>()
                .is_empty(),
            "a row naming `tool_on` must be caught: the wire carries \
             `authorized` and no `tool_on` at all"
        );
    }

    /// A field added to the wire and not to the row.
    #[test]
    fn a_published_field_missing_from_the_row_fails() {
        let mqtt = read(EDGE_MQTT).replace(
            r#"serde_json::json!({ "authorized": authorized, "reason": reason });"#,
            r#"serde_json::json!({ "authorized": authorized, "reason": reason, "retry_after_ms": 500 });"#,
        );
        let actual = published_json_keys(&fn_body(&mqtt, "handle_tool_on"));
        assert!(
            actual.contains("retry_after_ms"),
            "the mutant did not apply; the literal's text has changed and this \
             self-test is no longer driving the arm it names"
        );

        let documented = documented_payload_fields(&read(DOC), "toolguard/response/tool-on");
        assert!(
            !actual
                .difference(&documented)
                .collect::<Vec<_>>()
                .is_empty(),
            "a field published and not documented must be caught"
        );
    }

    /// The vacuity arm: a document whose local section is gone must fail rather
    /// than pass with an empty documented set.
    #[test]
    fn a_missing_table_is_not_a_pass() {
        let doc = read(DOC).replace(LOCAL_SECTION, "### Something else entirely");
        for (topic, _) in LOCAL_RESPONSE_PAYLOADS {
            assert!(
                documented_payload_fields(&doc, topic).is_empty(),
                "with the section heading gone, `{topic}` must yield no \
                 documented fields -- which is what the check's non-empty \
                 assertion then refuses"
            );
        }
    }

    /// The scoping arm: a handler's non-reply JSON must stay out of the set.
    /// `handle_tool_on` also builds the HTTP forward body `{card, tool_id}`,
    /// and counting those would make the row's comparison fail for fields that
    /// never touch the local broker.
    #[test]
    fn other_json_in_the_same_handler_is_not_collected() {
        let keys = published_json_keys(&fn_body(&read(EDGE_MQTT), "handle_tool_on"));
        assert!(
            keys.contains("authorized") && keys.contains("reason"),
            "the reply's own fields must be collected, got {keys:?}"
        );
        assert!(
            !keys.contains("card") && !keys.contains("tool_id"),
            "the HTTP forward body's fields must NOT be collected, got {keys:?}"
        );
    }
}
