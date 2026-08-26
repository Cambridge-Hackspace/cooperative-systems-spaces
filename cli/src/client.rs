use anyhow::{Context, Result};
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::config::CliConfig;

/// API client for communicating with the CSS server
pub struct ApiClient {
    client: Client,
    base_url: Url,
    token: Option<String>,
    log_requests: bool,
}

impl ApiClient {
    pub fn new(config: &CliConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .context("Failed to create HTTP client")?;

        let base_url = Url::parse(&config.server_url)
            .with_context(|| format!("Invalid server URL: {}", config.server_url))?;

        Ok(Self {
            client,
            base_url,
            token: config.auth_token.clone(),
            log_requests: config.log_requests,
        })
    }

    /// Make a GET request
    pub async fn get<T>(&self, path: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        let mut request = self.client.get(url);
        request = self.add_auth_header(request);

        let response = self.send_request(request).await?;
        self.parse_response(response).await
    }

    /// Make a POST request
    pub async fn post<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
        B: Serialize,
    {
        let url = self.build_url(path)?;
        let mut request = self.client.post(url).json(body);
        request = self.add_auth_header(request);

        let response = self.send_request(request).await?;
        self.parse_response(response).await
    }

    /// Make a PUT request
    pub async fn put<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
        B: Serialize,
    {
        let url = self.build_url(path)?;
        let mut request = self.client.put(url).json(body);
        request = self.add_auth_header(request);

        let response = self.send_request(request).await?;
        self.parse_response(response).await
    }

    /// Make a DELETE request
    pub async fn delete<T>(&self, path: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        let mut request = self.client.delete(url);
        request = self.add_auth_header(request);

        let response = self.send_request(request).await?;
        self.parse_response(response).await
    }

    /// Make a request and return raw response (for health checks, etc.)
    pub async fn request_raw(&self, method: Method, path: &str) -> Result<Response> {
        let url = self.build_url(path)?;
        let mut request = self.client.request(method, url);
        request = self.add_auth_header(request);

        self.send_request(request).await
    }

    fn build_url(&self, path: &str) -> Result<Url> {
        let path = path.strip_prefix('/').unwrap_or(path);

        // `Url::join` is RFC 3986 relative resolution, in which the final
        // segment of the base is a *file* unless the base ends in a slash. So
        // joining `api/auth/me` onto `https://example.com/css` resolves to
        // `https://example.com/api/auth/me` and the deployment's path prefix
        // is silently discarded.
        //
        // Nothing noticed because the default `server_url` has no path at all.
        // It fires the first time CSS is served under a prefix, which is an
        // entirely ordinary way to deploy it.
        let mut base = self.base_url.clone();
        if !base.path().ends_with('/') {
            let with_slash = format!("{}/", base.path());
            base.set_path(&with_slash);
        }

        base.join(path)
            .with_context(|| format!("Failed to build URL for path: {}", path))
    }

    fn add_auth_header(&self, mut builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }
        builder
    }

    async fn send_request(&self, builder: reqwest::RequestBuilder) -> Result<Response> {
        let request = builder.build().context("Failed to build request")?;

        if self.log_requests {
            tracing::info!("{} {}", request.method(), request.url());
        }

        let response = self.client
            .execute(request)
            .await
            .context("Failed to send request")?;

        if self.log_requests {
            tracing::info!("Response: {}", response.status());
        }

        Ok(response)
    }

    async fn parse_response<T>(&self, response: Response) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let status = response.status();
        let url = response.url().clone();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            
            return Err(anyhow::anyhow!(
                "Request failed with status {}: {} (URL: {})",
                status,
                error_text,
                url
            ));
        }

        let text = response
            .text()
            .await
            .context("Failed to read response body")?;

        if text.trim().is_empty() {
            // Handle empty responses for some endpoints
            return serde_json::from_value(serde_json::Value::Null)
                .context("Failed to parse empty response");
        }

        serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse JSON response: {}", text))
    }
}

/// Standard API response format
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// Paginated response format
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: u32,
    pub total_pages: u32,
}

/// User response model
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserResponse {
    pub id: uuid::Uuid,
    pub username: String,
    pub email: String,
    pub full_name: String,
    pub is_active: bool,
    pub role: UserRole,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

/// User role enum.
///
/// This is a fourth independent copy of a vocabulary that also exists in
/// `server/src/models.rs`, `frontend/src/types/index.ts` and the
/// `user_role` SQL enum — and it had drifted. It carried
/// `#[serde(rename_all = "lowercase")]`, while the server's copy has no
/// `rename_all` at all and therefore emits `"Admin"`. So this type could not
/// deserialize a role the server sent, which took the whole enclosing
/// `UserResponse` down with it: every command that lists or shows a user was
/// broken against its own server.
///
/// The lowercase form is not imaginary — it is what `ToSql` writes to
/// Postgres (`server/src/models.rs:49-58`) — but that is the *storage*
/// encoding, not the wire encoding, and this type is on the wire.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum UserRole {
    Unknown,
    Newbie,
    Member,
    Staff,
    Admin,
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRole::Unknown => write!(f, "Unknown"),
            UserRole::Newbie => write!(f, "Newbie"),
            UserRole::Member => write!(f, "Member"),
            UserRole::Staff => write!(f, "Staff"),
            UserRole::Admin => write!(f, "Admin"),
        }
    }
}

/// Login request
#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub username_or_email: String,
    pub password: String,
}

/// Login response
#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserResponse,
    pub expires_in: i64,
}

/// Registration request
#[derive(Debug, Serialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub full_name: String,
}

/// User update request
#[derive(Debug, Serialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub email: Option<String>,
    pub full_name: Option<String>,
    pub password: Option<String>,
    pub is_active: Option<bool>,
    pub role: Option<UserRole>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CliConfig;

    fn client_for(server_url: &str) -> ApiClient {
        ApiClient::new(&CliConfig {
            server_url: server_url.to_string(),
            ..CliConfig::default()
        })
        .expect("a well-formed URL and a default reqwest client build offline")
    }

    /// The shape every caller uses today, and the one that has always worked.
    #[test]
    fn build_url_joins_onto_a_bare_host() {
        let c = client_for("http://localhost:4399");
        assert_eq!(
            c.build_url("/api/auth/login").unwrap().as_str(),
            "http://localhost:4399/api/auth/login"
        );
    }

    #[test]
    fn build_url_accepts_a_path_with_or_without_its_leading_slash() {
        let c = client_for("http://localhost:4399");
        assert_eq!(
            c.build_url("api/auth/me").unwrap().as_str(),
            c.build_url("/api/auth/me").unwrap().as_str()
        );
    }

    /// `Url::join` is RFC 3986 relative resolution, in which the last segment
    /// of the base is a *file* unless it ends in a slash. So joining `users`
    /// onto `https://example.com/css` yields `https://example.com/users` and
    /// the deployment's path prefix vanishes -- every request 404s, or worse
    /// reaches something else entirely.
    ///
    /// Nothing catches this today because the default `server_url` has no path.
    /// It fires the moment CSS is served under a prefix, which is an ordinary
    /// way to deploy it.
    #[test]
    fn build_url_preserves_a_base_path_prefix() {
        let c = client_for("https://example.com/css");
        assert_eq!(
            c.build_url("/api/auth/me").unwrap().as_str(),
            "https://example.com/css/api/auth/me"
        );
    }

    #[test]
    fn build_url_is_stable_when_the_base_already_ends_in_a_slash() {
        let c = client_for("https://example.com/css/");
        assert_eq!(
            c.build_url("/api/auth/me").unwrap().as_str(),
            "https://example.com/css/api/auth/me"
        );
    }

    #[test]
    fn auth_header_is_bearer_and_absent_when_there_is_no_token() {
        let with = ApiClient::new(&CliConfig {
            auth_token: Some("t0ken".to_string()),
            ..CliConfig::default()
        })
        .unwrap();
        let req = with
            .add_auth_header(with.client.get("http://localhost:4399/"))
            .build()
            .unwrap();
        assert_eq!(req.headers()["authorization"], "Bearer t0ken");

        let without = client_for("http://localhost:4399");
        let req = without
            .add_auth_header(without.client.get("http://localhost:4399/"))
            .build()
            .unwrap();
        assert!(!req.headers().contains_key("authorization"));
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    /// The exact JSON the server emits for a role, and the exact JSON it will
    /// accept back.
    ///
    /// Written out here rather than imported from the server. That is
    /// deliberate on two counts: css-server cannot be compiled on this
    /// project's development workstation at all, and a check derived from the
    /// structure it is checking agrees with itself no matter what either side
    /// says. The duplication *is* the check.
    ///
    /// The authority is `server/src/models.rs:33-46`, whose `UserRole` derives
    /// `Serialize`/`Deserialize` with **no** `rename_all`, so serde emits the
    /// variant names verbatim. `frontend/src/types/index.ts:22-28` mirrors the
    /// same PascalCase strings, which corroborates it independently.
    const SERVER_WIRE: &[(&str, UserRole)] = &[
        ("Unknown", UserRole::Unknown),
        ("Newbie", UserRole::Newbie),
        ("Member", UserRole::Member),
        ("Staff", UserRole::Staff),
        ("Admin", UserRole::Admin),
    ];

    #[test]
    fn user_role_deserializes_what_the_server_actually_sends() {
        for (wire, expected) in SERVER_WIRE {
            let json = format!("\"{wire}\"");
            let got: UserRole = serde_json::from_str(&json).unwrap_or_else(|e| {
                panic!(
                    "the server sends {json} for this role and the CLI cannot read it: {e}. \
                     A UserResponse carrying it therefore fails to parse, so every command \
                     that lists or shows a user is broken against its own server."
                )
            });
            assert_eq!(got, *expected);
        }
    }

    #[test]
    fn user_role_serializes_to_what_the_server_accepts() {
        for (wire, role) in SERVER_WIRE {
            assert_eq!(serde_json::to_string(role).unwrap(), format!("\"{wire}\""));
        }
    }

    /// The whole payload, not just the field, because a role that fails to
    /// parse takes the entire `UserResponse` down with it and that is how this
    /// would actually be experienced.
    #[test]
    fn a_realistic_user_payload_round_trips() {
        let payload = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "username": "ada",
            "email": "ada@example.com",
            "full_name": "Ada Lovelace",
            "is_active": true,
            "role": "Staff",
            "created_at": "2026-01-15T12:00:00",
            "updated_at": "2026-01-15T12:00:00"
        }"#;
        let user: UserResponse = serde_json::from_str(payload).expect("server-shaped payload");
        assert_eq!(user.role, UserRole::Staff);
        assert_eq!(user.username, "ada");
    }
}
