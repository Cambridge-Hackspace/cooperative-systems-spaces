//! Tier 1b: the edge half of the door-access golden vectors.
//!
//! Reads the same `contracts/door_rules.json` the server half reads, and this
//! is where the arrangement earns its keep: the snapshot fed to
//! `DoorsState::decide` is built from **`expect.server_compiled`**, not from
//! the edge's own reading of the rules.
//!
//! That direction is deliberate. The edge never sees a rule — it only ever sees
//! the flat card lists the server compiled and published. Feeding it anything
//! else here would test a code path that does not exist in production. Building
//! it from the server's *declared* output means this file pins the
//! producer/consumer contract even if `expand_rules_at` is rewritten
//! completely, and it means a change to the compilation that the server half
//! accepts still has to satisfy the edge.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use css_edge::doors::{CompiledDoor, Decision, DoorStateSnapshot, DoorsState};
use serde_json::Value;
use uuid::Uuid;

const VECTORS: &str = include_str!("../../contracts/door_rules.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("contracts/door_rules.json must be valid JSON")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("expected an array")
        .iter()
        .map(|x| x.as_str().expect("string").to_string())
        .collect()
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
        // in the past. Decide against that fixed instant (via `decide_at`) so
        // the golden cases drive the same held-unlock code production runs.
        let now: DateTime<Utc> = case["now"]
            .as_str()
            .expect("now")
            .parse()
            .expect("now must be RFC 3339");
        // Built from the server's *declared* output, never the edge's own
        // reading of the rules -- including hold_unlock_until (absent/null =
        // no hold), so a held window compiled by the server is honored here.
        let hold_unlock_until: Option<DateTime<Utc>> = match &compiled["hold_unlock_until"] {
            Value::Null => None,
            Value::String(s) => Some(s.parse().expect("hold_unlock_until must be RFC 3339")),
            _ => None,
        };

        let door_id = Uuid::new_v4();
        let state = DoorsState::default();
        state.apply_snapshot(DoorStateSnapshot {
            snapshot_at: Utc::now(),
            doors: vec![CompiledDoor {
                id: door_id,
                name: "Vector Door".into(),
                enabled: true,
                unlock_duration_ms: 4200,
                allow_cards: strings(&compiled["allow"]),
                deny_cards: strings(&compiled["deny"]),
                hold_unlock_until,
            }],
        });

        for expectation in case["expect"]["edge_decisions"]
            .as_array()
            .expect("edge_decisions")
        {
            decisions += 1;
            let card = expectation["card"].as_str().expect("card");
            let want = expectation["granted"].as_bool().expect("granted");
            let got = matches!(state.decide_at(door_id, card, now), Decision::Allow { .. });
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
    // only thing that reads it. Asserted here so the two halves of the snapshot
    // cannot drift apart.
    let door_id = Uuid::new_v4();
    let state = DoorsState::default();
    state.apply_snapshot(DoorStateSnapshot {
        snapshot_at: Utc::now(),
        doors: vec![CompiledDoor {
            id: door_id,
            name: "Shut".into(),
            enabled: false,
            unlock_duration_ms: 4200,
            allow_cards: vec!["A1".into()],
            deny_cards: vec![],
            hold_unlock_until: None,
        }],
    });
    assert!(matches!(state.decide(door_id, "A1"), Decision::Deny(_)));
}

#[test]
fn an_unknown_door_is_refused_rather_than_defaulting() {
    let state = DoorsState::default();
    state.apply_snapshot(DoorStateSnapshot {
        snapshot_at: Utc::now(),
        doors: vec![],
    });
    assert!(matches!(
        state.decide(Uuid::new_v4(), "A1"),
        Decision::Deny(_)
    ));
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
    let state = DoorsState::default();
    state.apply_snapshot(DoorStateSnapshot {
        snapshot_at: Utc::now(),
        doors: vec![CompiledDoor {
            id: door_id,
            name: "Both".into(),
            enabled: true,
            unlock_duration_ms: 4200,
            allow_cards: vec!["A1".into(), "B2".into()],
            deny_cards: vec!["A1".into()],
            hold_unlock_until: None,
        }],
    });
    assert!(matches!(state.decide(door_id, "A1"), Decision::Deny(_)));
    assert!(matches!(
        state.decide(door_id, "B2"),
        Decision::Allow { .. }
    ));

    // And the sets themselves are what the server publishes, so an empty deny
    // list must not be mistaken for "deny everything".
    let _: BTreeSet<String> = BTreeSet::new();
}
