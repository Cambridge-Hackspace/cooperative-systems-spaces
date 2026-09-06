//! Groups.io API client.
//!
//! A thin async wrapper over the Groups.io v1 API used by the mailing-list
//! sync: direct-add and bulk-remove members, and page through the current and
//! past rosters. Following `mail.rs` / `recaptcha.rs`, request *building* is
//! pure and unit-tested (endpoint url + form/query fields), and only the
//! `send_*` methods touch the network -- so the shape of every call is checked
//! without a socket.
//!
//! Auth is an API key sent as a bearer token (Groups.io models this on Stripe).
//! Config is read live from [`ConfigManager`], so an admin reload re-points the
//! client without a restart.
//!
//! Retries follow the `webhooks.rs` shape: up to [`MAX_ATTEMPTS`] with
//! exponential backoff, retried only on 429 / 5xx / transport errors (Groups.io
//! answers overload with 429 and asks for backoff).
//!
//! NOTE: the exact Groups.io parameter names, member-status vocabulary, and the
//! group-id form (numeric id vs subdomain) are settled against a live account.
//! They are centralized in the pure builders and the `Raw*` types here so a
//! correction is a one-place edit; the e2e sink (`css-groupsio-sink`) accepts
//! the names this client sends.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::config::ConfigManager;

/// Maximum number of attempts per API call.
const MAX_ATTEMPTS: u32 = 3;
/// Per-request HTTP timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Page size when listing members (Groups.io caps this at 100).
const PAGE_LIMIT: u32 = 100;

/// One member of the group, reduced to what the sync needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    pub email: String,
    /// Groups.io membership status string (e.g. normal / bounced / no-email),
    /// captured verbatim. Not interpreted by the sync today; kept for
    /// diagnostics and future filtering.
    pub status: Option<String>,
}

/// Errors from a Groups.io API call.
#[derive(Debug, thiserror::Error)]
pub enum GroupsioError {
    #[error("Groups.io integration is disabled")]
    Disabled,
    #[error("Groups.io configuration error: {0}")]
    Configuration(String),
    #[error("Network error talking to Groups.io: {0}")]
    Network(String),
    #[error("Groups.io returned status {status}: {body}")]
    Api { status: u16, body: String },
    #[error("Failed to parse Groups.io response: {0}")]
    Parse(String),
}

/// The live settings a call needs, resolved from config at call time.
struct Settings {
    base_url: String,
    group_id: String,
    api_key: String,
}

#[derive(Clone)]
pub struct GroupsioClient {
    http: reqwest::Client,
    /// Read live rather than snapshotted, so `admin_config_reload` re-points the
    /// client on the next call instead of at the next restart.
    config: Arc<ConfigManager>,
}

impl GroupsioClient {
    pub fn new(config: Arc<ConfigManager>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                warn!("Failed to build Groups.io HTTP client ({e}); using default");
                reqwest::Client::new()
            });
        Self { http, config }
    }

    /// Resolve the settings a call needs, refusing early if the module is off or
    /// misconfigured (the caller only runs when enabled, but a live reload could
    /// have flipped it between the tick and the call).
    fn settings(&self) -> Result<Settings, GroupsioError> {
        let c = self.config.get_config();
        let g = c.groupsio;
        if !g.enabled {
            return Err(GroupsioError::Disabled);
        }
        if g.api_key.trim().is_empty() {
            return Err(GroupsioError::Configuration("api_key is empty".to_string()));
        }
        if g.group_id.trim().is_empty() {
            return Err(GroupsioError::Configuration(
                "group_id is empty".to_string(),
            ));
        }
        Ok(Settings {
            base_url: g.base_url,
            group_id: g.group_id,
            api_key: g.api_key,
        })
    }

    /// Direct-add members to the group. A no-op for an empty batch (so callers
    /// need not guard). Emails are sent as one newline-separated field, which is
    /// the `directadd` bulk shape.
    pub async fn direct_add(&self, emails: &[String]) -> Result<(), GroupsioError> {
        if emails.is_empty() {
            return Ok(());
        }
        let s = self.settings()?;
        let url = endpoint(&s.base_url, "directadd");
        let form = direct_add_form(&s.group_id, emails);
        debug!("Groups.io directadd: {} address(es)", emails.len());
        self.send_retrying(|| self.http.post(&url).bearer_auth(&s.api_key).form(&form))
            .await
            .map(|_| ())
    }

    /// Bulk-remove members from the group. A no-op for an empty batch.
    pub async fn remove_members(&self, emails: &[String]) -> Result<(), GroupsioError> {
        if emails.is_empty() {
            return Ok(());
        }
        let s = self.settings()?;
        let url = endpoint(&s.base_url, "bulkremovemembers");
        let form = remove_form(&s.group_id, emails);
        debug!("Groups.io bulkremovemembers: {} address(es)", emails.len());
        self.send_retrying(|| self.http.post(&url).bearer_auth(&s.api_key).form(&form))
            .await
            .map(|_| ())
    }

    /// The group's current members, following pagination to the end.
    pub async fn get_members(&self) -> Result<Vec<GroupMember>, GroupsioError> {
        self.list("getmembers").await
    }

    /// The group's past members -- people who left, unsubscribed (including from
    /// an email link), or were removed. The reconciler reads this to learn about
    /// an opt-out performed outside the platform.
    pub async fn get_past_members(&self) -> Result<Vec<GroupMember>, GroupsioError> {
        self.list("getpastmembers").await
    }

    /// Page through a member-listing endpoint until `has_more` is false.
    async fn list(&self, path: &str) -> Result<Vec<GroupMember>, GroupsioError> {
        let s = self.settings()?;
        let url = endpoint(&s.base_url, path);
        let mut out: Vec<GroupMember> = Vec::new();
        let mut page_token: Option<i64> = None;
        loop {
            let query = list_query(&s.group_id, PAGE_LIMIT, page_token);
            let page: MemberListResponse = self.get_json(&url, &s.api_key, &query).await?;
            for m in page.data {
                if !m.email.is_empty() {
                    out.push(GroupMember {
                        email: m.email,
                        status: m.status.or(m.user_status),
                    });
                }
            }
            if !page.has_more || page.next_page_token == 0 {
                break;
            }
            page_token = Some(page.next_page_token);
        }
        Ok(out)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        api_key: &str,
        query: &[(String, String)],
    ) -> Result<T, GroupsioError> {
        let resp = self
            .send_retrying(|| self.http.get(url).bearer_auth(api_key).query(query))
            .await?;
        let text = resp
            .text()
            .await
            .map_err(|e| GroupsioError::Network(e.to_string()))?;
        serde_json::from_str::<T>(&text).map_err(|e| GroupsioError::Parse(e.to_string()))
    }

    /// Send a freshly-built request, retrying on 429 / 5xx / transport errors
    /// with exponential backoff. `build` yields a new `RequestBuilder` per
    /// attempt because a builder is not reusable once sent.
    async fn send_retrying<F>(&self, build: F) -> Result<reqwest::Response, GroupsioError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut last = GroupsioError::Network("no attempts made".to_string());
        for attempt in 1..=MAX_ATTEMPTS {
            match build().send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    let body = resp.text().await.unwrap_or_default();
                    last = GroupsioError::Api {
                        status: status.as_u16(),
                        body,
                    };
                    if !retryable || attempt == MAX_ATTEMPTS {
                        return Err(last);
                    }
                    warn!(
                        "Groups.io call attempt {}/{} got retryable status {}",
                        attempt, MAX_ATTEMPTS, status
                    );
                }
                Err(e) => {
                    last = GroupsioError::Network(e.to_string());
                    if attempt == MAX_ATTEMPTS {
                        return Err(last);
                    }
                    warn!(
                        "Groups.io call attempt {}/{} failed: {}",
                        attempt, MAX_ATTEMPTS, e
                    );
                }
            }
            tokio::time::sleep(backoff(attempt)).await;
        }
        Err(last)
    }
}

/// Exponential backoff between attempts: 1s, 2s, ... matching `webhooks.rs`.
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1 << (attempt - 1))
}

/// Join a base URL and a path segment, tolerating a trailing slash on the base.
fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}

/// Form fields for `directadd`: the group and a newline-separated address list.
fn direct_add_form(group_id: &str, emails: &[String]) -> Vec<(String, String)> {
    vec![
        ("group_id".to_string(), group_id.to_string()),
        ("emails".to_string(), emails.join("\n")),
    ]
}

/// Form fields for `bulkremovemembers`: the group and a newline-separated list.
fn remove_form(group_id: &str, emails: &[String]) -> Vec<(String, String)> {
    vec![
        ("group_id".to_string(), group_id.to_string()),
        ("emails".to_string(), emails.join("\n")),
    ]
}

/// Query fields for a member-listing call: the group, a page size, and a page
/// token once the first page has told us there is another.
fn list_query(group_id: &str, limit: u32, page_token: Option<i64>) -> Vec<(String, String)> {
    let mut q = vec![
        ("group_id".to_string(), group_id.to_string()),
        ("limit".to_string(), limit.to_string()),
    ];
    if let Some(token) = page_token {
        q.push(("page_token".to_string(), token.to_string()));
    }
    q
}

#[derive(Debug, Deserialize)]
struct MemberListResponse {
    #[serde(default)]
    data: Vec<RawMember>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_page_token: i64,
}

#[derive(Debug, Deserialize)]
struct RawMember {
    #[serde(default)]
    email: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    user_status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_joins_and_trims_a_trailing_slash() {
        assert_eq!(
            endpoint("https://groups.io/api/v1", "directadd"),
            "https://groups.io/api/v1/directadd"
        );
        assert_eq!(
            endpoint("https://groups.io/api/v1/", "getmembers"),
            "https://groups.io/api/v1/getmembers"
        );
    }

    #[test]
    fn direct_add_sends_the_group_and_a_newline_joined_list() {
        let form = direct_add_form(
            "g1",
            &["a@example.org".to_string(), "b@example.org".to_string()],
        );
        assert_eq!(
            form,
            vec![
                ("group_id".to_string(), "g1".to_string()),
                (
                    "emails".to_string(),
                    "a@example.org\nb@example.org".to_string()
                ),
            ]
        );
    }

    #[test]
    fn remove_sends_the_group_and_a_newline_joined_list() {
        let form = remove_form("g1", &["gone@example.org".to_string()]);
        assert_eq!(
            form,
            vec![
                ("group_id".to_string(), "g1".to_string()),
                ("emails".to_string(), "gone@example.org".to_string()),
            ]
        );
    }

    #[test]
    fn list_query_omits_the_page_token_on_the_first_page() {
        let q = list_query("g1", 100, None);
        assert_eq!(
            q,
            vec![
                ("group_id".to_string(), "g1".to_string()),
                ("limit".to_string(), "100".to_string()),
            ]
        );
    }

    #[test]
    fn list_query_carries_the_page_token_on_later_pages() {
        let q = list_query("g1", 100, Some(42));
        assert_eq!(
            q,
            vec![
                ("group_id".to_string(), "g1".to_string()),
                ("limit".to_string(), "100".to_string()),
                ("page_token".to_string(), "42".to_string()),
            ]
        );
    }

    #[test]
    fn backoff_is_exponential() {
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(2));
        assert_eq!(backoff(3), Duration::from_secs(4));
    }

    #[test]
    fn a_member_page_parses_and_keeps_status() {
        let json = r#"{
            "data": [
                {"email": "a@example.org", "status": "normal"},
                {"email": "b@example.org", "user_status": "bouncing"}
            ],
            "has_more": true,
            "next_page_token": 7
        }"#;
        let page: MemberListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(page.data.len(), 2);
        assert_eq!(page.data[0].email, "a@example.org");
        assert_eq!(page.data[0].status.as_deref(), Some("normal"));
        assert_eq!(page.data[1].user_status.as_deref(), Some("bouncing"));
        assert!(page.has_more);
        assert_eq!(page.next_page_token, 7);
    }

    #[test]
    fn a_member_page_tolerates_missing_optional_fields() {
        // An older/short response with no pagination fields must not fail to
        // parse -- it means "one page, no more".
        let page: MemberListResponse =
            serde_json::from_str(r#"{"data": [{"email": "a@example.org"}]}"#).unwrap();
        assert_eq!(page.data.len(), 1);
        assert!(!page.has_more);
        assert_eq!(page.next_page_token, 0);
        assert!(page.data[0].status.is_none());
    }
}
