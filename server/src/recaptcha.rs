use serde::{Deserialize, Serialize};
use tracing::{debug, error};

/// reCAPTCHA verification response from Google's API
#[derive(Debug, Serialize, Deserialize)]
struct RecaptchaResponse {
    success: bool,
    #[serde(rename = "error-codes")]
    error_codes: Option<Vec<String>>,
    challenge_ts: Option<String>,
    hostname: Option<String>,
    score: Option<f64>,
    action: Option<String>,
}

/// reCAPTCHA verification service
#[derive(Debug, Clone)]
pub struct RecaptchaService {
    secret_key: String,
    client: reqwest::Client,
}

impl RecaptchaService {
    /// Create a new reCAPTCHA service instance
    pub fn new(secret_key: String) -> Self {
        Self {
            secret_key,
            client: reqwest::Client::new(),
        }
    }

    /// Verify a reCAPTCHA response token
    /// Returns Ok(true) if verification succeeds, Ok(false) if it fails, Err for network/API errors
    pub async fn verify_token(&self, token: &str, remote_ip: Option<&str>) -> Result<bool, RecaptchaError> {
        if self.secret_key.is_empty() {
            error!("reCAPTCHA secret key is empty");
            return Err(RecaptchaError::ConfigurationError("reCAPTCHA secret key not configured".to_string()));
        }

        if token.is_empty() {
            debug!("Empty reCAPTCHA token provided");
            return Ok(false);
        }

        let mut params = vec![
            ("secret", self.secret_key.as_str()),
            ("response", token),
        ];

        if let Some(ip) = remote_ip {
            params.push(("remoteip", ip));
        }

        debug!("Verifying reCAPTCHA token with Google API");

        let response = self
            .client
            .post("https://www.google.com/recaptcha/api/siteverify")
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to send reCAPTCHA verification request: {}", e);
                RecaptchaError::NetworkError(e.to_string())
            })?;

        let recaptcha_response: RecaptchaResponse = response
            .json()
            .await
            .map_err(|e| {
                error!("Failed to parse reCAPTCHA response: {}", e);
                RecaptchaError::ParseError(e.to_string())
            })?;

        debug!("reCAPTCHA verification response: {:?}", recaptcha_response);

        if recaptcha_response.success {
            debug!("reCAPTCHA verification successful");
            Ok(true)
        } else {
            if let Some(error_codes) = &recaptcha_response.error_codes {
                debug!("reCAPTCHA verification failed with error codes: {:?}", error_codes);
            }
            Ok(false)
        }
    }

    /// Verify a reCAPTCHA v3 token with score threshold
    /// Returns Ok(true) if verification succeeds and score is above threshold
    pub async fn verify_token_with_score(
        &self,
        token: &str,
        remote_ip: Option<&str>,
        min_score: f64,
    ) -> Result<bool, RecaptchaError> {
        if self.secret_key.is_empty() {
            error!("reCAPTCHA secret key is empty");
            return Err(RecaptchaError::ConfigurationError("reCAPTCHA secret key not configured".to_string()));
        }

        if token.is_empty() {
            debug!("Empty reCAPTCHA token provided");
            return Ok(false);
        }

        let mut params = vec![
            ("secret", self.secret_key.as_str()),
            ("response", token),
        ];

        if let Some(ip) = remote_ip {
            params.push(("remoteip", ip));
        }

        debug!("Verifying reCAPTCHA v3 token with score threshold: {}", min_score);

        let response = self
            .client
            .post("https://www.google.com/recaptcha/api/siteverify")
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to send reCAPTCHA verification request: {}", e);
                RecaptchaError::NetworkError(e.to_string())
            })?;

        let recaptcha_response: RecaptchaResponse = response
            .json()
            .await
            .map_err(|e| {
                error!("Failed to parse reCAPTCHA response: {}", e);
                RecaptchaError::ParseError(e.to_string())
            })?;

        debug!("reCAPTCHA v3 verification response: {:?}", recaptcha_response);

        if recaptcha_response.success {
            if let Some(score) = recaptcha_response.score {
                debug!("reCAPTCHA verification successful with score: {}", score);
                Ok(score >= min_score)
            } else {
                debug!("reCAPTCHA verification successful (no score provided, assuming v2)");
                Ok(true)
            }
        } else {
            if let Some(error_codes) = &recaptcha_response.error_codes {
                debug!("reCAPTCHA verification failed with error codes: {:?}", error_codes);
            }
            Ok(false)
        }
    }
}

/// reCAPTCHA verification errors
#[derive(Debug, thiserror::Error)]
pub enum RecaptchaError {
    #[error("reCAPTCHA configuration error: {0}")]
    ConfigurationError(String),

    #[error("Network error during reCAPTCHA verification: {0}")]
    NetworkError(String),

    #[error("Failed to parse reCAPTCHA response: {0}")]
    ParseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recaptcha_service_creation() {
        let service = RecaptchaService::new("test_secret_key".to_string());
        assert_eq!(service.secret_key, "test_secret_key");
    }

    #[tokio::test]
    async fn test_verify_empty_token() {
        let service = RecaptchaService::new("test_secret_key".to_string());
        let result = service.verify_token("", None).await.unwrap();
        assert!(!result); // Empty token should return false
    }

    #[tokio::test]
    async fn test_verify_empty_secret_key() {
        let service = RecaptchaService::new(String::new());
        let result = service.verify_token("some_token", None).await;
        assert!(matches!(result, Err(RecaptchaError::ConfigurationError(_))));
    }
}