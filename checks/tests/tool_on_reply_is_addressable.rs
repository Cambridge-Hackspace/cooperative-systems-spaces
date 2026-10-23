//! A tool-on reply must name the tool it answers about (#166).
//!
//! `toolguard/response/tool-on` is unnamespaced and carries no device id, so
//! every module on a local broker receives every answer. Until #166 the payload
//! was `{ authorized, reason }` -- nothing identifying what it was a reply to --
//! and it was built as a `serde_json::json!` literal in *three* places inside
//! `handle_tool_on`: the power-lockout deny, the online-synchronous path, and
//! the cached-allow-list path.
//!
//! A module therefore could not tell its own verdict from the verdict for the
//! reader at the next machine. Two readers on one broker, and a yes at one
//! starts a session at the other where somebody is waiting for a no. The
//! firmware-side mitigation -- accept a reply only while exactly one request is
//! outstanding -- narrows the window and cannot close it, which is why
//! `spaces-firmware` shipped with "one module per broker, for now".
//!
//! The door half of the same resource model (#101) never had this problem:
//! `door/response/unlock` has always echoed `door_id`.
//!
//! The fix is structural, and so is this check. The reply is a type
//! (`css_lib::wire::LocalToolOnResponse`) published from a single helper, so no
//! decision path can answer without `tool_id`. Three `json!` literals could
//! drift apart; one constructor cannot, and the compiler rejects an omission.
//!
//! A text-level check on purpose: it needs no broker and no compiler, so it runs
//! on the FreeBSD workstation where `css-server` cannot be built at all.
//!
//! What this does NOT prove: that any receiver actually filters on it, or that
//! the edge's authorization decisions are right. Whether firmware checks it is
//! firmware's own oracle; the rule is stated at `FIRMWARE.md`'s tool-on
//! walkthrough.
//!
//! WHY A SOURCE CHECK AND NOT AN e2e ASSERTION. Nothing in `e2e/` subscribes to
//! `toolguard/response/*` -- the battery's only MQTT publish is
//! `toolguard/request/power` -- so a full green battery cannot observe this
//! payload at all. That is worth stating where it will be read: the battery
//! passing is not evidence about this topic, and until a driver watches the
//! reply, these checks and the firmware's own host tests are the whole of the
//! coverage.

use css_checks::read;

const EDGE_MQTT: &str = "edge/src/mqtt.rs";
const WIRE: &str = "css_lib/src/wire.rs";

/// Source with `//` line comments stripped.
///
/// Both directions matter. The prose above and in `mqtt.rs` explains the defect
/// by naming the construct being banned, so an unstripped scan would read the
/// explanation as the offence; and a presence assertion could otherwise be
/// satisfied by a comment that merely mentions the type it wants.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every complaint about how the edge answers tool-on. Empty means well-formed.
///
/// A function over a string rather than assertions inline, so the oracle itself
/// can be driven against the shape this defect actually had -- see
/// `the_check_fires_on_the_shape_that_shipped`. A check never observed
/// complaining is a check whose value is unmeasured.
fn faults(edge_source: &str) -> Vec<String> {
    let code = code_only(edge_source);
    let mut out = Vec::new();

    // One publish site. Three was the defect's shape: each a separate literal,
    // each able to forget a field the others carry.
    let sites = code.matches("local::TOOL_ON_RESPONSE").count();
    if sites != 1 {
        out.push(format!(
            "{EDGE_MQTT} publishes local::TOOL_ON_RESPONSE from {sites} places, want exactly 1 \
             (the `publish_tool_on` helper). Separate sites drift: before #166 there were three, \
             and none of them carried tool_id."
        ));
    }

    // And that site must build the type, not a literal. This is what makes the
    // omission a compile error rather than a review question.
    if !code.contains("LocalToolOnResponse") {
        out.push(format!(
            "{EDGE_MQTT} does not construct LocalToolOnResponse. A `json!` literal here can omit \
             tool_id silently, which is exactly how the unaddressable reply shipped."
        ));
    }

    // AND THE ECHOED ID MUST BE THE REQUESTED ONE. A single publish site through
    // a typed payload makes omission impossible; it does not stop the helper
    // being handed the wrong variable, which would produce a reply that is
    // well-formed, consistently wrong, and addressed to a tool nobody asked
    // about. Every call must pass the request's own tool_id.
    let calls: Vec<&str> = code
        .match_indices("publish_tool_on(")
        .map(|(i, _)| {
            let rest = &code[i..];
            let end = rest.find(')').map(|e| e + 1).unwrap_or(rest.len());
            &rest[..end]
        })
        .collect();
    // The definition itself matches the needle; the call sites are the rest.
    let sites: Vec<&&str> = calls
        .iter()
        .filter(|c| !c.contains("tool_id: &str"))
        .collect();
    if sites.is_empty() {
        out.push(format!(
            "{EDGE_MQTT} has no publish_tool_on call sites at all -- the helper is \
             defined and never used, so the tool-on reply is published by something else."
        ));
    }
    for site in &sites {
        if !site.contains("&req.tool_id") {
            out.push(format!(
                "a publish_tool_on call does not pass the request's own tool_id: {site}\n\
                 The reply would name a tool the caller did not ask about, which is \
                 indistinguishable to a reader from a reply meant for its neighbour."
            ));
        }
    }

    out
}

/// The serde field names of a `pub struct` declared at module level.
///
/// Sliced to the closing brace at the declaration's own indentation. Closing on
/// the first `}` found would stop at the first nested type or block instead.
fn struct_fields(source: &str, name: &str) -> Vec<String> {
    let decl = format!("pub struct {name} {{");
    let start = match source.find(&decl) {
        Some(i) => i + decl.len(),
        None => return Vec::new(),
    };
    let body = match source[start..].find("\n}") {
        Some(i) => &source[start..start + i],
        None => &source[start..],
    };
    code_only(body)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("pub ")?;
            let (field, _) = line.split_once(':')?;
            Some(field.trim().to_string())
        })
        .collect()
}

#[test]
fn the_tool_on_reply_is_published_through_one_typed_helper() {
    let found = faults(&read(EDGE_MQTT));
    assert!(
        found.is_empty(),
        "the tool-on reply is not structurally addressable:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_reply_type_carries_the_tool_id() {
    let fields = struct_fields(&read(WIRE), "LocalToolOnResponse");

    assert!(
        !fields.is_empty(),
        "LocalToolOnResponse not found in {WIRE}. It is the canonical tool-on reply; if it was \
         renamed, this check and the row in \
         checks/tests/the_firmware_protocol_is_documented.rs both need the new name."
    );
    assert!(
        fields.contains(&"tool_id".to_string()),
        "LocalToolOnResponse has no tool_id field, so the reply names nothing it answers. \
         Fields found: {fields:?}"
    );
    assert!(
        fields.contains(&"authorized".to_string()),
        "LocalToolOnResponse has no authorized field -- the verdict itself. Fields: {fields:?}"
    );
}

/// The oracle, pointed at the defect it exists to catch.
///
/// This fixture is the pre-#166 shape, reduced: two of the three literals, each
/// carrying `authorized` and `reason` and neither carrying `tool_id`. If
/// `faults` does not complain about this, it would not have complained about
/// what shipped either, and the two tests above are decoration.
#[test]
fn the_check_fires_on_the_shape_that_shipped() {
    let before = r#"
    async fn handle_tool_on(&self, req: LocalToolRequest) {
        if self.power_state.is_locked(&req.tool_id) {
            let response_payload = serde_json::json!({
                "authorized": false,
                "reason": "Tool is locked out (power)"
            });
            self.publish_local(local::TOOL_ON_RESPONSE, &response_payload);
            return;
        }
        let response_payload = serde_json::json!({ "authorized": authorized, "reason": reason });
        self.publish_local(local::TOOL_ON_RESPONSE, &response_payload);
    }
"#;

    let found = faults(before);

    assert!(
        found.iter().any(|f| f.contains("from 2 places")),
        "the oracle did not notice multiple publish sites. Faults: {found:?}"
    );
    assert!(
        found.iter().any(|f| f.contains("LocalToolOnResponse")),
        "the oracle did not notice the untyped literal. Faults: {found:?}"
    );

    // And the control, so "fires" is not "fires at everything": the shape the
    // fix produces must come back clean through the same function.
    // The control must be the WHOLE fixed shape, definition and call site both.
    // With only the definition it tripped the "defined and never used" fault --
    // caught by this very arm, which is the self-test earning its place: an
    // incomplete control would have read as the oracle being wrong.
    let after = r#"
    async fn handle_tool_on(&self, req: LocalToolRequest) {
        if self.power_state.is_locked(&req.tool_id) {
            self.publish_tool_on(&req.tool_id, false, "Tool is locked out (power)");
            return;
        }
    }

    fn publish_tool_on(&self, tool_id: &str, authorized: bool, reason: &str) {
        let payload = LocalToolOnResponse {
            tool_id: tool_id.to_string(),
            authorized,
            reason: reason.to_string(),
        };
        match serde_json::to_value(&payload) {
            Ok(v) => self.publish_local(local::TOOL_ON_RESPONSE, &v),
            Err(e) => warn!("Failed to serialize tool-on response: {}", e),
        }
    }
"#;
    assert!(
        faults(after).is_empty(),
        "the oracle complains about the fixed shape: {:?}",
        faults(after)
    );
}

/// `struct_fields` is the other half of this oracle and fails toward
/// reassurance: a rename makes it return nothing, and nothing contains no
/// missing field. The emptiness assertion above is what catches that, so drive
/// both cells here.
#[test]
fn the_field_reader_can_tell_present_from_absent() {
    let src = r#"
pub struct LocalToolOnResponse {
    pub tool_id: String,
    pub authorized: bool,
    pub reason: String,
}

pub struct Unrelated {
    pub other: u8,
}
"#;
    assert_eq!(
        struct_fields(src, "LocalToolOnResponse"),
        vec!["tool_id", "authorized", "reason"],
        "field reader did not read the fields it was pointed at"
    );
    assert!(
        struct_fields(src, "NoSuchStruct").is_empty(),
        "field reader invented fields for a struct that is not there"
    );
    assert!(
        !struct_fields(src, "LocalToolOnResponse").contains(&"other".to_string()),
        "field reader ran past the struct's own closing brace into the next one"
    );
}
