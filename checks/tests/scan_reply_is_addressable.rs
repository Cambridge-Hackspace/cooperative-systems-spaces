//! A scan reply must name the resource it answers about (#166, #192).
//!
//! `resource/response/scan` is unnamespaced and carries no device id, so every
//! module on a local broker receives every answer. Until #166 the tool-on
//! reply this replaced was `{ authorized, reason }` -- nothing identifying what
//! it was a reply to -- and it was built as a `serde_json::json!` literal in
//! *three* places inside `handle_tool_on`: the power-lockout deny, the
//! online-synchronous path, and the cached-allow-list path.
//!
//! A module therefore could not tell its own verdict from the verdict for the
//! reader at the next machine. Two readers on one broker, and a yes at one
//! starts a session at the other where somebody is waiting for a no. The
//! firmware-side mitigation -- accept a reply only while exactly one request is
//! outstanding -- narrows the window and cannot close it, which is why
//! `spaces-firmware` shipped with "one module per broker, for now".
//!
//! The fix is structural, and so is this check. The reply is a type
//! (`css_lib::wire::LocalScanReply`) built through one constructor and
//! published from a single helper, so no decision path -- a scan's answer, a
//! server-initiated unlock, an Open Access hold pulse -- can answer without
//! `resource`. Three `json!` literals could drift apart; one constructor
//! cannot, and the compiler rejects an omission.
//!
//! A text-level check on purpose: it needs no broker and no compiler, so it runs
//! on the FreeBSD workstation where `css-server` cannot be built at all.
//!
//! What this does NOT prove: that any receiver actually filters on it, or that
//! the edge's authorization decisions are right. Whether firmware checks it is
//! firmware's own oracle; the rule is stated at `FIRMWARE.md`'s scan
//! walkthrough. The battery's `toolonreply` driver watches the reply on a real
//! broker and asserts the echo varies with the request, so the two tiers meet.
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

/// Every complaint about how the edge answers a scan. Empty means well-formed.
///
/// A function over a string rather than assertions inline, so the oracle itself
/// can be driven against the shape this defect actually had -- see
/// `the_check_fires_on_the_shape_that_shipped`. A check never observed
/// complaining is a check whose value is unmeasured.
fn faults(edge_source: &str) -> Vec<String> {
    let code = code_only(edge_source);
    let mut out = Vec::new();

    // One publish site. Three was the defect's shape: each a separate literal,
    // each able to forget a field the others carry. Since #192 that one site
    // answers every unlock too -- a server command, a hold pulse -- so there is
    // nothing left to drift.
    let sites = code.matches("local::SCAN_RESPONSE").count();
    if sites != 1 {
        out.push(format!(
            "{EDGE_MQTT} publishes local::SCAN_RESPONSE from {sites} places, want exactly 1 \
             (the `publish_scan_reply` helper). Separate sites drift: before #166 there were \
             three, and none of them carried the resource."
        ));
    }

    // And that site must build the type through its constructor, not a literal.
    // This is what makes the omission a compile error rather than a review
    // question, and the constructor is what drops the strike on a refusal.
    if !code.contains("LocalScanReply::new(") {
        out.push(format!(
            "{EDGE_MQTT} does not construct the reply through LocalScanReply::new. A `json!` \
             literal here can omit the resource silently, which is exactly how the \
             unaddressable reply shipped."
        ));
    }

    // AND THE ECHOED NAME MUST BE THE REQUESTED ONE. A single publish site
    // through a typed payload makes omission impossible; it does not stop the
    // helper being handed the wrong variable, which would produce a reply that
    // is well-formed, consistently wrong, and addressed to a resource nobody
    // asked about. Every call must pass the request's own resource -- or, for
    // an unlock nobody asked for, the door the command or the hold names.
    let calls: Vec<&str> = code
        .match_indices("publish_scan_reply(")
        .map(|(i, _)| {
            let rest = &code[i..];
            let end = rest.find(')').map(|e| e + 1).unwrap_or(rest.len());
            &rest[..end]
        })
        .collect();
    // The definition itself matches the needle; the call sites are the rest.
    let sites: Vec<&&str> = calls
        .iter()
        .filter(|c| !c.contains("resource: &str"))
        .collect();
    if sites.is_empty() {
        out.push(format!(
            "{EDGE_MQTT} has no publish_scan_reply call sites at all -- the helper is \
             defined and never used, so the scan reply is published by something else."
        ));
    }
    for site in &sites {
        let first = site
            .split_once('(')
            .map(|(_, rest)| rest)
            .unwrap_or("")
            .split(',')
            .next()
            .unwrap_or("")
            .trim();
        if !(first.contains("req.resource") || first.contains("door_id")) {
            out.push(format!(
                "a publish_scan_reply call does not pass the request's own resource: {site}\n\
                 The reply would name a resource the caller did not ask about, which is \
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
fn the_scan_reply_is_published_through_one_typed_helper() {
    let found = faults(&read(EDGE_MQTT));
    assert!(
        found.is_empty(),
        "the scan reply is not structurally addressable:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_reply_type_carries_the_resource() {
    let fields = struct_fields(&read(WIRE), "LocalScanReply");

    assert!(
        !fields.is_empty(),
        "LocalScanReply not found in {WIRE}. It is the canonical scan reply; if it was \
         renamed, this check and the row in \
         checks/tests/the_firmware_protocol_is_documented.rs both need the new name."
    );
    assert!(
        fields.contains(&"resource".to_string()),
        "LocalScanReply has no resource field, so the reply names nothing it answers. \
         Fields found: {fields:?}"
    );
    assert!(
        fields.contains(&"authorized".to_string()),
        "LocalScanReply has no authorized field -- the verdict itself. Fields: {fields:?}"
    );
}

/// The oracle, pointed at the defect it exists to catch.
///
/// This fixture is the pre-#166 shape, reduced and spelled on today's topic:
/// two of the three literals, each carrying `authorized` and `reason` and
/// neither carrying the resource. If `faults` does not complain about this, it
/// would not have complained about what shipped either, and the two tests
/// above are decoration.
#[test]
fn the_check_fires_on_the_shape_that_shipped() {
    let before = r#"
    async fn handle_scan(&self, req: LocalScan) {
        if self.power_state.is_locked(&req.resource) {
            let response_payload = serde_json::json!({
                "authorized": false,
                "reason": "Tool is locked out (power)"
            });
            self.publish_local(local::SCAN_RESPONSE, &response_payload);
            return;
        }
        let response_payload = serde_json::json!({ "authorized": authorized, "reason": reason });
        self.publish_local(local::SCAN_RESPONSE, &response_payload);
    }
"#;

    let found = faults(before);

    assert!(
        found.iter().any(|f| f.contains("from 2 places")),
        "the oracle did not notice multiple publish sites. Faults: {found:?}"
    );
    assert!(
        found.iter().any(|f| f.contains("LocalScanReply::new")),
        "the oracle did not notice the untyped literal. Faults: {found:?}"
    );

    // A single typed site handed the wrong name: well-formed, consistently
    // wrong, addressed to a resource nobody asked about.
    let wrong_name = r#"
    async fn handle_scan(&self, req: LocalScan) {
        self.publish_scan_reply(&self.last_resource, authorized, &reason, None);
    }

    fn publish_scan_reply(&self, resource: &str, authorized: bool, reason: &str, pulse_ms: Option<i32>) {
        let reply = LocalScanReply::new(resource, authorized, reason, pulse_ms);
        match serde_json::to_value(&reply) {
            Ok(v) => self.publish_local(local::SCAN_RESPONSE, &v),
            Err(e) => warn!("Failed to serialize scan reply: {}", e),
        }
    }
"#;
    assert!(
        faults(wrong_name)
            .iter()
            .any(|f| f.contains("does not pass the request's own resource")),
        "the oracle did not notice a reply addressed to the wrong resource: {:?}",
        faults(wrong_name)
    );

    // And the control, so "fires" is not "fires at everything": the shape the
    // fix produces must come back clean through the same function.
    // The control must be the WHOLE fixed shape, definition and call sites both.
    // With only the definition it tripped the "defined and never used" fault --
    // caught by this very arm, which is the self-test earning its place: an
    // incomplete control would have read as the oracle being wrong.
    let after = r#"
    async fn handle_scan(&self, req: LocalScan) {
        if self.power_state.is_locked(&req.resource) {
            self.publish_scan_reply(&req.resource, false, "Tool is locked out (power)", None);
            return;
        }
    }

    pub fn publish_doors_hold(&self, door_id: uuid::Uuid, duration_ms: i32) {
        self.publish_scan_reply(&door_id.to_string(), true, "open_access", Some(duration_ms));
    }

    fn publish_scan_reply(&self, resource: &str, authorized: bool, reason: &str, pulse_ms: Option<i32>) {
        let reply = LocalScanReply::new(resource, authorized, reason, pulse_ms);
        match serde_json::to_value(&reply) {
            Ok(v) => self.publish_local(local::SCAN_RESPONSE, &v),
            Err(e) => warn!("Failed to serialize scan reply: {}", e),
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
pub struct LocalScanReply {
    pub resource: String,
    pub authorized: bool,
    pub reason: String,
}

pub struct Unrelated {
    pub other: u8,
}
"#;
    assert_eq!(
        struct_fields(src, "LocalScanReply"),
        vec!["resource", "authorized", "reason"],
        "field reader did not read the fields it was pointed at"
    );
    assert!(
        struct_fields(src, "NoSuchStruct").is_empty(),
        "field reader invented fields for a struct that is not there"
    );
    assert!(
        !struct_fields(src, "LocalScanReply").contains(&"other".to_string()),
        "field reader ran past the struct's own closing brace into the next one"
    );
}
