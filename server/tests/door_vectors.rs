//! Tier 1b: the server half of the door-access golden vectors.
//!
//! Three implementations have to agree about who a door opens for:
//!
//! * `server/src/doors.rs::expand_rules_at` — compiles rules into the flat
//!   allow/deny card lists published to a device;
//! * `server/src/doors.rs::evaluate` — the QR check-in path, which decides for
//!   a known user rather than a card;
//! * `edge/src/doors.rs::DoorsState::decide` — decides an RFID scan locally,
//!   from the lists the first one produced.
//!
//! None of them is the oracle. `contracts/door_rules.json` is, and it is read
//! by this file and by `edge/tests/door_vectors.rs`. That is the whole point of
//! the arrangement: two self-consistent implementations that disagree with each
//! other is a failure no amount of testing either one alone can find, and it is
//! exactly what the last case in the file records.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use css_server::doors::{cards_in_profile, expand_rules_at, open_access_hold_until_at};
use css_server::models::{AccessRule, Schedule};
use serde_json::Value;
use uuid::Uuid;

const VECTORS: &str = include_str!("../../contracts/door_rules.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("contracts/door_rules.json must be valid JSON")
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or_else(|_| panic!("bad uuid in vectors: {s}"))
}

/// The seeded role graph (name -> level), matching the RBAC seed migration, so
/// door role-rule expansion resolves "this tier or higher" the same way the
/// server does. Only levels matter here; permissions/inheritance are irrelevant
/// to `level_of_name`.
fn seed_graph() -> css_server::rbac::RoleGraph {
    let rows: Vec<(Uuid, String, i16)> = [
        ("guest", 1),
        ("historical", 2),
        ("active", 3),
        ("staff", 4),
        ("admin", 5),
    ]
    .iter()
    .enumerate()
    .map(|(i, (name, level))| (Uuid::from_u128(i as u128 + 1), name.to_string(), *level))
    .collect();
    css_server::rbac::RoleGraph::from_rows(&rows, &[], &[])
}

fn rule_from(v: &Value) -> AccessRule {
    AccessRule {
        id: Uuid::new_v4(),
        resource_id: Uuid::nil(),
        kind: v["kind"].as_str().expect("kind").to_string(),
        value: v["value"].as_str().expect("value").to_string(),
        effect: v["effect"].as_str().expect("effect").to_string(),
        created_at: Utc::now(),
        schedule_id: v["schedule_id"].as_str().map(uuid),
    }
}

fn schedule_from(v: &Value) -> Schedule {
    Schedule {
        id: uuid(v["id"].as_str().expect("schedule id")),
        name: "vector".into(),
        description: None,
        intervals: v["intervals"].clone(),
        created_by: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        is_public: false,
    }
}

fn set(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .expect("expected an array")
        .iter()
        .map(|x| x.as_str().expect("string").to_string())
        .collect()
}

/// An optional RFC 3339 instant from the vectors: absent or JSON `null` means
/// "no hold" (`None`); anything else must parse.
fn opt_dt(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.parse().expect("hold_unlock_until must be RFC 3339")),
        other => panic!("hold_unlock_until must be a string or null, got {other}"),
    }
}

#[test]
fn every_case_compiles_to_the_declared_card_sets() {
    let doc = vectors();
    let cases = doc["cases"].as_array().expect("cases");

    let mut failures = Vec::new();

    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let now: DateTime<Utc> = case["now"]
            .as_str()
            .expect("now")
            .parse()
            .expect("now must be RFC 3339");
        let tz: chrono_tz::Tz = case["tz"].as_str().expect("tz").parse().expect("tz");

        // #120 (#122/H3+H4): expand_rules_at now consumes per-user card *tokens*
        // (wire-digests in production) rather than reading profiles. The vectors
        // carry opaque card tokens, so feed each active user's fixture `cards`
        // directly -- the routing logic under test is identical. Only active
        // users reach compilation (compile_state_for sources list_active_users),
        // which is what makes the inactive-user vector case mean anything.
        let cards_by_user: std::collections::HashMap<Uuid, Vec<String>> = case["users"]
            .as_array()
            .expect("users")
            .iter()
            .filter(|u| u["is_active"].as_bool().unwrap_or(false))
            .map(|u| {
                let id = uuid(u["id"].as_str().expect("user id"));
                let cards: Vec<String> = u["cards"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| c.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                (id, cards)
            })
            .collect();

        let rules: Vec<AccessRule> = case["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .map(rule_from)
            .collect();
        let schedules: Vec<Schedule> = case["schedules"]
            .as_array()
            .expect("schedules")
            .iter()
            .map(schedule_from)
            .collect();

        // Effective tier level per active user, resolved from the vector's role
        // string against the seed (the User struct no longer carries a role).
        let graph = seed_graph();
        let user_levels: std::collections::HashMap<Uuid, i16> = case["users"]
            .as_array()
            .expect("users")
            .iter()
            .filter(|u| u["is_active"].as_bool().unwrap_or(false))
            .map(|u| {
                (
                    uuid(u["id"].as_str().expect("user id")),
                    graph
                        .level_of_name(u["role"].as_str().expect("role"))
                        .unwrap_or(0),
                )
            })
            .collect();

        let (allow, deny) = expand_rules_at(
            &rules,
            &cards_by_user,
            &schedules,
            tz,
            now,
            &graph,
            &user_levels,
        );
        let hold = open_access_hold_until_at(&rules, &schedules, tz, now);

        let want = &case["expect"]["server_compiled"];
        let (want_allow, want_deny) = (set(&want["allow"]), set(&want["deny"]));
        // Absent key = no hold, which also pins that card-only rules never
        // compile a held-unlock window.
        let want_hold = opt_dt(&want["hold_unlock_until"]);

        if allow != want_allow {
            failures.push(format!(
                "{name}: allow {allow:?} != expected {want_allow:?}"
            ));
        }
        if deny != want_deny {
            failures.push(format!("{name}: deny {deny:?} != expected {want_deny:?}"));
        }
        if hold != want_hold {
            failures.push(format!(
                "{name}: hold_unlock_until {hold:?} != expected {want_hold:?}"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_vector_file_was_actually_read() {
    // Guards the guard: an empty or unparsed file would make the assertions
    // above iterate over nothing and pass.
    let doc = vectors();
    let cases = doc["cases"].as_array().expect("cases");
    assert!(cases.len() >= 10, "only {} vector cases", cases.len());

    // And the cases that carry the sharpest claims are present by name, so a
    // future edit cannot quietly drop them while leaving the count intact.
    let names: Vec<&str> = cases
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect::<Vec<_>>();
    for needle in [
        "unrecognized effect is skipped",
        "deny beats allow",
        "schedule-gated rule is silent",
        "inactive member",
        "open access holds the door",
        "deny open-access rule is inert",
        "banned card stays out even during open access",
    ] {
        assert!(
            names.iter().any(|n| n.contains(needle)),
            "the vector case about '{needle}' is gone; it recorded a real defect"
        );
    }
}

#[test]
fn may_reproduces_the_per_principal_door_decision() {
    // #101 slice 2a: the unified engine must reproduce `DoorService::evaluate`'s
    // per-principal answer. The vectors are the oracle: each case's `decisions`
    // array carries hand-authored expected outcomes (allow/deny) for a named user,
    // and we run them through `access_engine::may` -- independent of `may`'s own
    // unit tests and of `expand_rules_at`. Door policy is Restricted (no default
    // grant); the interesting variation is per-user role level, user/card match,
    // and the inactive-member divergence (compiled into the card set, but denied
    // to the user in person).
    use css_server::access_engine::{may, Action, DefaultEffect, Principal, ResourcePolicy};

    let doc = vectors();
    let cases = doc["cases"].as_array().expect("cases");
    let graph = seed_graph();
    let mut failures = Vec::new();
    let mut checked = 0usize;

    for case in cases {
        let Some(decisions) = case.get("decisions").and_then(|d| d.as_array()) else {
            continue;
        };
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let now: DateTime<Utc> = case["now"]
            .as_str()
            .expect("now")
            .parse()
            .expect("now rfc3339");
        let tz: chrono_tz::Tz = case["tz"].as_str().expect("tz").parse().expect("tz");
        let rules: Vec<AccessRule> = case["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .map(rule_from)
            .collect();
        let schedules: Vec<Schedule> = case["schedules"]
            .as_array()
            .expect("schedules")
            .iter()
            .map(schedule_from)
            .collect();
        let users = case["users"].as_array().expect("users");

        for dec in decisions {
            let pid = dec["principal"].as_str().expect("principal");
            let action = match dec["action"].as_str().expect("action") {
                "unlock" => Action::Unlock,
                "use" => Action::Use,
                "unlock_remote" => Action::UnlockRemote,
                other => panic!("unknown action {other} in case {name}"),
            };
            let want_allow = match dec["expect"].as_str().expect("expect") {
                "allow" => true,
                "deny" => false,
                other => panic!("expect must be allow|deny, got {other}"),
            };
            let u = users
                .iter()
                .find(|u| u["id"].as_str() == Some(pid))
                .unwrap_or_else(|| panic!("decision names user {pid} not in case {name}"));
            let level = graph
                .level_of_name(u["role"].as_str().expect("role"))
                .unwrap_or(0);
            let card_digests: BTreeSet<String> = u["cards"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let principal = Principal {
                user_id: Some(uuid(pid)),
                level,
                card_digests,
                is_active: u["is_active"].as_bool().unwrap_or(false),
            };
            // These are all door cases: a door is Restricted and carries no
            // resource-level schedule (its schedules live on the rules).
            let policy = ResourcePolicy {
                locked_out: false,
                unavailable: None,
                default_effect: DefaultEffect::Restricted,
                schedule_id: None,
                training_ok: None,
                metered_ok: None,
            };
            let got = may(
                &principal, &policy, &rules, &schedules, tz, now, &graph, action,
            );
            if got.is_allow() != want_allow {
                failures.push(format!(
                    "{name}: principal {}.. -> {got:?}, expected {}",
                    &pid[..8],
                    if want_allow { "allow" } else { "deny" }
                ));
            }
            checked += 1;
        }
    }

    // Guard the guard: the decisions must actually be present and exercised.
    assert!(
        checked >= 6,
        "expected >= 6 per-principal decisions, checked {checked}"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cards_are_read_from_both_profile_shapes() {
    // The profile field holds either a scalar string (the original shape) or an
    // array (the TextArray shape). Both are live in deployed data.
    let field = "rfid_card";
    assert_eq!(
        cards_in_profile(&serde_json::json!({ field: "A1" }), field),
        vec!["A1".to_string()]
    );
    assert_eq!(
        cards_in_profile(&serde_json::json!({ field: ["A1", "B2"] }), field),
        vec!["A1".to_string(), "B2".to_string()]
    );
    // Empty values are not cards. An empty string in allow_cards would match an
    // empty scan.
    assert!(cards_in_profile(&serde_json::json!({ field: "" }), field).is_empty());
    assert!(cards_in_profile(&serde_json::json!({ field: ["", "B2"] }), field) == vec!["B2"]);
    assert!(cards_in_profile(&serde_json::json!({ field: 42 }), field).is_empty());
    assert!(cards_in_profile(&serde_json::json!({}), field).is_empty());
}
