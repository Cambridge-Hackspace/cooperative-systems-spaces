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

use chrono::{DateTime, NaiveDateTime, Utc};
use css_server::doors::{cards_in_profile, expand_rules_at};
use css_server::models::{DoorAccessRule, Schedule, User, UserRole};
use serde_json::Value;
use uuid::Uuid;

const VECTORS: &str = include_str!("../../contracts/door_rules.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("contracts/door_rules.json must be valid JSON")
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or_else(|_| panic!("bad uuid in vectors: {s}"))
}

fn role(s: &str) -> UserRole {
    match s {
        "unknown" => UserRole::Unknown,
        "newbie" => UserRole::Newbie,
        "member" => UserRole::Member,
        "staff" => UserRole::Staff,
        "admin" => UserRole::Admin,
        other => panic!("unknown role in vectors: {other}"),
    }
}

fn epoch() -> NaiveDateTime {
    DateTime::from_timestamp(0, 0).expect("epoch").naive_utc()
}

fn user_from(v: &Value, profile_field: &str) -> User {
    let cards: Vec<Value> = v["cards"]
        .as_array()
        .expect("cards must be an array")
        .clone();
    User {
        id: uuid(v["id"].as_str().expect("user id")),
        username: "vector".into(),
        email: "vector@example.com".into(),
        password_hash: String::new(),
        full_name: "Vector User".into(),
        is_active: v["is_active"].as_bool().expect("is_active"),
        created_at: epoch(),
        updated_at: epoch(),
        role: role(v["role"].as_str().expect("role")),
        profile: serde_json::json!({ profile_field: cards }),
        meta: Value::Null,
        mfa_enrolled_at: None,
    }
}

fn rule_from(v: &Value) -> DoorAccessRule {
    DoorAccessRule {
        id: Uuid::new_v4(),
        door_id: Uuid::nil(),
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

#[test]
fn every_case_compiles_to_the_declared_card_sets() {
    let doc = vectors();
    let profile_field = doc["profile_field"].as_str().expect("profile_field");
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

        // Only active users reach compilation: `compile_state_for` sources them
        // from `list_active_users()`. Modelling that here is what makes the
        // inactive-user case in the vectors mean anything.
        let users: Vec<User> = case["users"]
            .as_array()
            .expect("users")
            .iter()
            .map(|u| user_from(u, profile_field))
            .filter(|u| u.is_active)
            .collect();

        let rules: Vec<DoorAccessRule> = case["rules"]
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

        let (allow, deny) = expand_rules_at(&rules, &users, &schedules, tz, profile_field, now);

        let want = &case["expect"]["server_compiled"];
        let (want_allow, want_deny) = (set(&want["allow"]), set(&want["deny"]));

        if allow != want_allow {
            failures.push(format!(
                "{name}: allow {allow:?} != expected {want_allow:?}"
            ));
        }
        if deny != want_deny {
            failures.push(format!("{name}: deny {deny:?} != expected {want_deny:?}"));
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
    ] {
        assert!(
            names.iter().any(|n| n.contains(needle)),
            "the vector case about '{needle}' is gone; it recorded a real defect"
        );
    }
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
