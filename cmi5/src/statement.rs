//! The xAPI statement model — the slice cmi5 exchanges.
//!
//! These are deserialized from the JSON the content POSTs to the LRS and
//! serialized when the LMS issues its own statements (`launched`, `satisfied`).
//! We model only what cmi5 uses: an actor (an account-based Agent), a verb, an
//! Activity object, an optional result with a score, and a context carrying the
//! registration and the cmi5/moveon category activities. Unknown fields are
//! preserved on the wire by the server storing the raw JSON alongside; the typed
//! view here is what validation reasons over.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// xAPI verb IRIs. cmi5 fixes the exact IRI for each verb, so these are compared
/// as opaque strings, never by a display label (which is advisory and localized).
pub mod verbs {
    pub const LAUNCHED: &str = "http://adlnet.gov/expapi/verbs/launched";
    pub const INITIALIZED: &str = "http://adlnet.gov/expapi/verbs/initialized";
    pub const COMPLETED: &str = "http://adlnet.gov/expapi/verbs/completed";
    pub const PASSED: &str = "http://adlnet.gov/expapi/verbs/passed";
    pub const FAILED: &str = "http://adlnet.gov/expapi/verbs/failed";
    pub const TERMINATED: &str = "http://adlnet.gov/expapi/verbs/terminated";
    pub const SATISFIED: &str = "https://w3id.org/xapi/adl/verbs/satisfied";
    pub const WAIVED: &str = "https://w3id.org/xapi/adl/verbs/waived";
    pub const ABANDONED: &str = "https://w3id.org/xapi/adl/verbs/abandoned";
}

/// cmi5 context category activity IRIs.
pub mod categories {
    /// Marks a statement as a cmi5 "defined" statement.
    pub const CMI5: &str = "https://w3id.org/xapi/cmi5/context/categories/cmi5";
    /// Marks a completed/passed/failed statement as counting toward `moveOn`.
    pub const MOVEON: &str = "https://w3id.org/xapi/cmi5/context/categories/moveon";
}

/// An xAPI actor. cmi5 requires an account-based Agent; `mbox`/`name` are modeled
/// so a malformed actor deserializes rather than erroring, and validation rejects
/// it explicitly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    #[serde(
        rename = "objectType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub object_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
}

/// An account-based actor identity: `{homePage, name}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    #[serde(rename = "homePage")]
    pub home_page: String,
    pub name: String,
}

/// An xAPI verb: an IRI plus advisory display labels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verb {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<BTreeMap<String, String>>,
}

/// The statement object. cmi5 statements are about an Activity (the AU).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StatementObject {
    Activity(Activity),
}

impl StatementObject {
    /// The activity IRI, if the object is an activity.
    pub fn activity_id(&self) -> &str {
        match self {
            StatementObject::Activity(a) => &a.id,
        }
    }
}

/// An xAPI Activity: an IRI and an optional definition (which we do not inspect).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    #[serde(
        rename = "objectType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub object_type: Option<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<serde_json::Value>,
}

/// An xAPI result. cmi5 gates on `success` and `score.scaled`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct XResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<Score>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
}

/// An xAPI score. `scaled` is the normalized 0.0..=1.0 value cmi5 compares to
/// `masteryScore`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Score {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scaled: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// xAPI context. cmi5 puts the `registration` here and the cmi5/moveon markers in
/// `contextActivities.category`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<Uuid>,
    #[serde(
        rename = "contextActivities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub context_activities: Option<ContextActivities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Value>,
}

/// The `contextActivities` sub-object. Only `category` matters to cmi5 gating.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ContextActivities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Vec<Activity>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Vec<Activity>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<Vec<Activity>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<Vec<Activity>>,
}

impl ContextActivities {
    /// Whether the category list contains an activity with the given IRI.
    pub fn has_category(&self, iri: &str) -> bool {
        self.category
            .as_ref()
            .map(|cats| cats.iter().any(|a| a.id == iri))
            .unwrap_or(false)
    }
}

/// An xAPI statement, the cmi5 slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub actor: Agent,
    pub verb: Verb,
    pub object: StatementObject,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<XResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

impl Statement {
    /// The verb IRI.
    pub fn verb_id(&self) -> &str {
        &self.verb.id
    }

    /// The activity IRI the statement is about.
    pub fn object_activity_id(&self) -> &str {
        self.object.activity_id()
    }

    /// The registration UUID from context, if present.
    pub fn registration(&self) -> Option<Uuid> {
        self.context.as_ref().and_then(|c| c.registration)
    }

    /// Whether the statement carries the moveon category activity.
    pub fn is_moveon(&self) -> bool {
        self.context
            .as_ref()
            .and_then(|c| c.context_activities.as_ref())
            .map(|ca| ca.has_category(categories::MOVEON))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passed_json(scaled: f64) -> String {
        format!(
            r#"{{
                "id": "6d4b1e2a-1111-2222-3333-444455556666",
                "actor": {{"objectType":"Agent","account":{{"homePage":"https://space.example","name":"user-uuid-1"}}}},
                "verb": {{"id":"http://adlnet.gov/expapi/verbs/passed"}},
                "object": {{"objectType":"Activity","id":"http://example.com/au/1"}},
                "result": {{"success":true,"score":{{"scaled":{scaled}}}}},
                "context": {{
                    "registration":"00000000-0000-0000-0000-0000000000aa",
                    "contextActivities": {{"category":[{{"id":"https://w3id.org/xapi/cmi5/context/categories/moveon"}}]}}
                }}
            }}"#
        )
    }

    #[test]
    fn deserializes_a_cmi5_passed_statement() {
        let s: Statement = serde_json::from_str(&passed_json(0.9)).expect("parse");
        assert_eq!(s.verb_id(), verbs::PASSED);
        assert_eq!(s.object_activity_id(), "http://example.com/au/1");
        assert_eq!(s.actor.account.as_ref().unwrap().name, "user-uuid-1");
        assert_eq!(s.result.as_ref().unwrap().success, Some(true));
        assert_eq!(
            s.result.as_ref().unwrap().score.as_ref().unwrap().scaled,
            Some(0.9)
        );
        assert!(s.is_moveon());
        assert_eq!(
            s.registration().unwrap().to_string(),
            "00000000-0000-0000-0000-0000000000aa"
        );
    }

    #[test]
    fn round_trips_a_statement_through_json() {
        let s: Statement = serde_json::from_str(&passed_json(0.75)).expect("parse");
        let json = serde_json::to_string(&s).expect("serialize");
        let again: Statement = serde_json::from_str(&json).expect("reparse");
        assert_eq!(s, again);
    }

    #[test]
    fn a_statement_without_moveon_category_is_not_moveon() {
        let json = r#"{
            "actor": {"account":{"homePage":"h","name":"n"}},
            "verb": {"id":"http://adlnet.gov/expapi/verbs/completed"},
            "object": {"id":"http://example.com/au/1"}
        }"#;
        let s: Statement = serde_json::from_str(json).expect("parse");
        assert!(!s.is_moveon());
        assert_eq!(s.registration(), None);
    }
}
