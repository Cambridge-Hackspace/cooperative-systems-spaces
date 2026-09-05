//! Launch-URL assembly and the `LMS.LaunchData` document.
//!
//! At launch the server appends a query string to the AU's `url` carrying the
//! five cmi5-mandated parameters, and stashes an `LMS.LaunchData` document the
//! content reads back through the State API. Both are built here, purely, so the
//! exact wire shape is unit-tested away from the HTTP layer.

use crate::manifest::MoveOn;
use crate::statement::Agent;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The cmi5 launch mode. Only `Normal` may satisfy moveOn and grant access;
/// `Browse`/`Review` are non-credit and the server must not translate them into
/// a completion (enforced server-side — this enum is what it records at launch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchMode {
    Normal,
    Browse,
    Review,
}

impl LaunchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LaunchMode::Normal => "Normal",
            LaunchMode::Browse => "Browse",
            LaunchMode::Review => "Review",
        }
    }

    /// Whether a session launched in this mode is allowed to count toward moveOn.
    pub fn is_creditable(self) -> bool {
        matches!(self, LaunchMode::Normal)
    }
}

/// The parameters the LMS injects into the AU launch URL.
#[derive(Debug, Clone)]
pub struct LaunchParams<'a> {
    /// Absolute URL of our LRS base (statements/state live under it).
    pub endpoint: &'a str,
    /// The one-time `fetch` URL the content trades for a session credential.
    pub fetch: &'a str,
    /// The learner as an xAPI Agent (account-based).
    pub actor: &'a Agent,
    /// The registration grouping this attempt.
    pub registration: Uuid,
    /// The AU's activity IRI.
    pub activity_id: &'a str,
}

/// Build the cmi5 launch query string (no leading `?`), with every value
/// percent-encoded. Parameter order is fixed for determinism (and so tests can
/// assert the whole string), though cmi5 does not require a particular order.
pub fn build_launch_query(params: &LaunchParams<'_>) -> Result<String, serde_json::Error> {
    let actor_json = serde_json::to_string(params.actor)?;
    Ok(format!(
        "endpoint={}&fetch={}&actor={}&activityId={}&registration={}",
        encode_component(params.endpoint),
        encode_component(params.fetch),
        encode_component(&actor_json),
        encode_component(params.activity_id),
        encode_component(&params.registration.to_string()),
    ))
}

/// Append a launch query to an AU `url`, choosing `?` or `&` as the URL requires.
pub fn append_query(au_url: &str, query: &str) -> String {
    let sep = if au_url.contains('?') { '&' } else { '?' };
    format!("{au_url}{sep}{query}")
}

/// The `LMS.LaunchData` state document the content fetches after launch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaunchData {
    #[serde(rename = "contextTemplate")]
    pub context_template: serde_json::Value,
    #[serde(rename = "launchMode")]
    pub launch_mode: LaunchMode,
    #[serde(rename = "moveOn")]
    pub move_on: MoveOn,
    #[serde(rename = "masteryScore", skip_serializing_if = "Option::is_none")]
    pub mastery_score: Option<f64>,
    #[serde(rename = "returnURL", skip_serializing_if = "Option::is_none")]
    pub return_url: Option<String>,
    #[serde(rename = "entitlementKey", skip_serializing_if = "Option::is_none")]
    pub entitlement_key: Option<String>,
}

impl LaunchData {
    /// Build launch data with a minimal `contextTemplate` carrying the cmi5
    /// category and the registration — the shape cmi5 requires the content to
    /// echo on its own statements.
    pub fn new(
        registration: Uuid,
        launch_mode: LaunchMode,
        move_on: MoveOn,
        mastery_score: Option<f64>,
    ) -> Self {
        let context_template = serde_json::json!({
            "registration": registration.to_string(),
            "contextActivities": {
                "category": [
                    { "id": crate::statement::categories::CMI5 }
                ]
            }
        });
        LaunchData {
            context_template,
            launch_mode,
            move_on,
            mastery_score,
            return_url: None,
            entitlement_key: None,
        }
    }
}

/// Percent-encode a string for use as a URL query component (RFC 3986: everything
/// outside the unreserved set is `%`-escaped).
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
    out
}

fn hex_digit(n: u8) -> char {
    match n & 0x0f {
        d @ 0..=9 => (b'0' + d) as char,
        d => (b'A' + (d - 10)) as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statement::{Account, Agent};

    fn actor() -> Agent {
        Agent {
            object_type: Some("Agent".into()),
            name: None,
            mbox: None,
            account: Some(Account {
                home_page: "https://space.example".into(),
                name: "learner-1".into(),
            }),
        }
    }

    #[test]
    fn launch_query_carries_all_mandated_params() {
        let a = actor();
        let params = LaunchParams {
            endpoint: "https://space.example/api/cmi5/lrs",
            fetch: "https://space.example/api/cmi5/fetch?token=abc",
            actor: &a,
            registration: Uuid::from_u128(0xAA),
            activity_id: "http://example.com/au/1",
        };
        let q = build_launch_query(&params).expect("build");
        for key in [
            "endpoint=",
            "fetch=",
            "actor=",
            "activityId=",
            "registration=",
        ] {
            assert!(q.contains(key), "missing {key} in {q}");
        }
        // Reserved characters must be encoded, not passed through raw.
        assert!(!q.contains("://api"), "endpoint should be encoded: {q}");
        assert!(
            q.contains("%3A%2F%2F"),
            "':' and '/' should be %-encoded: {q}"
        );
        // The actor JSON is embedded encoded; decoding proves it survived.
        let actor_enc = q.split("actor=").nth(1).unwrap().split('&').next().unwrap();
        let decoded = decode(actor_enc);
        let round: Agent = serde_json::from_str(&decoded).expect("decode actor");
        assert_eq!(round.account.unwrap().name, "learner-1");
    }

    #[test]
    fn append_query_picks_the_right_separator() {
        assert_eq!(append_query("a.html", "x=1"), "a.html?x=1");
        assert_eq!(append_query("a.html?y=2", "x=1"), "a.html?y=2&x=1");
    }

    #[test]
    fn launch_data_serializes_with_cmi5_field_names() {
        let ld = LaunchData::new(
            Uuid::from_u128(0xAA),
            LaunchMode::Normal,
            MoveOn::CompletedAndPassed,
            Some(0.8),
        );
        let v = serde_json::to_value(&ld).expect("serialize");
        assert_eq!(v["launchMode"], "Normal");
        assert_eq!(v["moveOn"], "CompletedAndPassed");
        assert_eq!(v["masteryScore"], 0.8);
        assert_eq!(
            v["contextTemplate"]["contextActivities"]["category"][0]["id"],
            crate::statement::categories::CMI5
        );
        // An omitted returnURL must not appear as null.
        assert!(v.get("returnURL").is_none());
    }

    #[test]
    fn browse_and_review_are_not_creditable() {
        assert!(LaunchMode::Normal.is_creditable());
        assert!(!LaunchMode::Browse.is_creditable());
        assert!(!LaunchMode::Review.is_creditable());
    }

    /// Minimal percent-decoder, test-only, to prove `encode_component` is
    /// reversible rather than merely non-empty.
    fn decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hi = (bytes[i + 1] as char).to_digit(16).unwrap() as u8;
                let lo = (bytes[i + 2] as char).to_digit(16).unwrap() as u8;
                out.push((hi << 4) | lo);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }
}
