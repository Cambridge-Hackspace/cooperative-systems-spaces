use anyhow::{Context, Result};
use reqwest::{Client, Method, Request, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

    /// Set the authentication token
    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    /// Clear the authentication token
    pub fn clear_token(&mut self) {
        self.token = None;
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
        let path = if path.starts_with('/') {
            &path[1..]
        } else {
            path
        };

        self.base_url
            .join(path)
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

/// User role enum
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
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