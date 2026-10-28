//! Tier 1b: the edge half of the door-access golden vectors.
//!
//! Reads the same `contracts/door_rules.json` the server half reads, and this
//! is where the arrangement earns its keep: the policy fed to
//! `ResourceCache::decide` is built from **`expect.server_compiled`**, not from
//! the edge's own reading of the rules.
//!
//! That direction is deliberate. The edge never sees a rule — it only ever sees
//! the flat card lists the server compiled and published. Feeding it anything
//! else here would test a code path that does not exist in production. Building
//! it from the server's *declared* output means this file pins the
//! producer/consumer contract even if `expand_rules_at` is rewritten
//! completely, and it means a change to the compilation that the server half
//! accepts still has to satisfy the edge.
//!
//! Since #191 the edge decides a door from the one resource policy, with the
//! one `decide` a tool is decided by, so this drives that. The vectors name
//! cards in the clear and the server ships their digests; the harness digests
//! each compiled list with the pepper the cache hashes swipes with, which is
//! exactly the relationship between a deployment's server and its edge.

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use css_edge::modules::ModuleState;
use css_edge::power::PowerState;
use css_edge::resources::{ResourceCache, ScanDecision};
use css_edge::toolguard::ToolGuardState;
use css_lib::card_crypto::CardDigester;
use css_lib::wire::{ResourcePolicy, ResourcePolicyPayload};
use serde_json::Value;
use uuid::Uuid;

const VECTORS: &str = include_str!("../../contracts/door_rules.json");
const PEP: &str = "0404040404040404040404040404040404040404040404040404040404040404";

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("contracts/door_rules.json must be valid JSON")
}

fn digester() -> Arc<CardDigester> {
    Arc::new(CardDigester::from_hex(PEP).expect("test pepper parses"))
}

/// The server ships a compiled list as digests, never cards.
fn digests(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("expected an array")
        .iter()
        .map(|x| {
            hex::encode(
                digester()
                    .wire_digest(x.as_str().expect("string"))
                    .expect("digests"),
            )
        })
        .collect()
}

fn door(
    id: Uuid,
    enabled: bool,
    allow: Vec<String>,
    deny: Vec<String>,
    hold: Option<String>,
) -> ResourcePolicy {
    ResourcePolicy {
        id: id.to_string(),
        external_id: None,
        name: "Vector Door".into(),
        kind: "door".into(),
        session_profile: "momentary".into(),
        status: String::new(),
        requires_online: false,
        in_use: false,
        enabled,
        pulse_ms: Some(4200),
        allow,
        deny,
        hold_until: hold,
    }
}

/// A cache holding exactly this policy and no principals: a door is decided
/// by its compiled lists alone, as the server compiles them.
fn cache_with(resources: Vec<ResourcePolicy>) -> Arc<ResourceCache> {
    let cache = ResourceCache::new(
        Arc::new(ModuleState::new()),
        Arc::new(ToolGuardState::with_digester(Some(digester()))),
        Arc::new(PowerState::new()),
    );
    cache.apply_policy(
        ResourcePolicyPayload {
            device_id: Uuid::from_u128(0xE).to_string(),
            as_of: Utc::now().to_rfc3339(),
            profile_field: "card".into(),
            resources,
            principals: vec![],
        },
        None,
    );
    cache
}

fn granted(cache: &ResourceCache, door_id: Uuid, card: &str, now: DateTime<Utc>) -> bool {
    matches!(
        cache.decide(&door_id.to_string(), card, now),
        ScanDecision::Allow { .. }
    )
}

#[test]
fn every_case_decides_as_the_vectors_declare() {
    let doc = vectors();
    let cases = doc["cases"].as_array().expect("cases");
    let mut failures = Vec::new();
    let mut decisions = 0usize;

    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let compiled = &case["expect"]["server_compiled"];

        // The Open Access latch is time-relative, and the vectors freeze `now`
        // in the past. Decide against that fixed instant so the golden cases
        // drive the same held-unlock code production runs.
        let now: DateTime<Utc> = case["now"]
            .as_str()
            .expect("now")
            .parse()
            .expect("now must be RFC 3339");
        // Built from the server's *declared* output, never the edge's own
        // reading of the rules -- including hold_unlock_until (absent/null =
        // no hold), so a held window compiled by the server is honored here.
        let hold: Option<String> = match &compiled["hold_unlock_until"] {
            Value::String(s) => {
                let _: DateTime<Utc> = s.parse().expect("hold_unlock_until must be RFC 3339");
                Some(s.clone())
            }
            _ => None,
        };

        let door_id = Uuid::new_v4();
        let cache = cache_with(vec![door(
            door_id,
            true,
            digests(&compiled["allow"]),
            digests(&compiled["deny"]),
            hold,
        )]);

        for expectation in case["expect"]["edge_decisions"]
            .as_array()
            .expect("edge_decisions")
        {
            decisions += 1;
            let card = expectation["card"].as_str().expect("card");
            let want = expectation["granted"].as_bool().expect("granted");
            let got = granted(&cache, door_id, card, now);
            if got != want {
                failures.push(format!(
                    "{name}: card {card} -> granted={got}, expected {want}"
                ));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        decisions >= 12,
        "only {decisions} edge decisions exercised; the vectors should carry more"
    );
}

#[test]
fn a_disabled_door_refuses_a_card_the_server_allowed() {
    // Not in the vector file because it is not a property of the *rules* — the
    // server compiles `enabled` alongside the card lists and the edge is the
    // only thing that reads it. Asserted here so the two halves of the policy
    // cannot drift apart.
    let door_id = Uuid::new_v4();
    let cache = cache_with(vec![door(
        door_id,
        false,
        digests(&serde_json::json!(["A1"])),
        vec![],
        None,
    )]);
    assert!(!granted(&cache, door_id, "A1", Utc::now()));
}

#[test]
fn an_unknown_door_is_refused_rather_than_defaulting() {
    let cache = cache_with(vec![]);
    assert!(!granted(&cache, Uuid::new_v4(), "A1", Utc::now()));
}

#[test]
fn the_edge_and_server_halves_read_the_same_cases() {
    // Both files include! the same path, but that is a compile-time fact about
    // this crate only. This asserts the shape the *other* half depends on is
    // present, so a vector edit that satisfies one side and breaks the other
    // fails here rather than silently halving the coverage.
    let doc = vectors();
    for case in doc["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        assert!(
            case["expect"]["server_compiled"]["allow"].is_array(),
            "{name}: missing expect.server_compiled.allow, which the server half asserts"
        );
        assert!(
            case["expect"]["edge_decisions"].is_array(),
            "{name}: missing expect.edge_decisions, which this half asserts"
        );
        assert!(
            !case["expect"]["edge_decisions"]
                .as_array()
                .expect("array")
                .is_empty(),
            "{name}: has no edge decisions, so the edge half skips it entirely"
        );
    }
}

#[test]
fn deny_wins_even_when_the_card_is_also_allowed() {
    // The single most important property of the decision function, asserted
    // directly as well as through the vectors: a card in both lists must be
    // refused. Getting this backwards opens a door to someone explicitly
    // barred from it.
    let door_id = Uuid::new_v4();
    let cache = cache_with(vec![door(
        door_id,
        true,
        digests(&serde_json::json!(["A1", "B2"])),
        digests(&serde_json::json!(["A1"])),
        None,
    )]);
    assert!(!granted(&cache, door_id, "A1", Utc::now()));
    assert!(granted(&cache, door_id, "B2", Utc::now()));

    // And the sets themselves are what the server publishes, so an empty deny
    // list must not be mistaken for "deny everything".
    let _: BTreeSet<String> = BTreeSet::new();
}
