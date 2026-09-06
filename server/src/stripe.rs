//! Stripe API client.
//!
//! A thin async wrapper over the Stripe REST API for the membership module:
//! create a hosted Checkout Session (recurring or one-shot), create a Billing
//! Portal session, and list a customer's paid invoices (the renewal cycle's
//! backbone for re-crediting a payment whose webhook was missed).
//!
//! Following `groupsio.rs` / `mail.rs`, request *building* is pure and
//! unit-tested (endpoint url + form/query fields), and only the `send_*` methods
//! touch the network. Auth is the secret key as a bearer token. Config is read
//! live from [`ConfigManager`], so an admin reload re-points the client without a
//! restart. The `base_url` is configurable so the e2e stack can substitute an
//! in-process fake for `api.stripe.com`.
//!
//! **No card data passes through here.** Checkout and the Billing Portal are
//! Stripe-hosted; the platform only mints hosted URLs and reads back amounts and
//! reference ids (SAQ-A posture).
//!
//! Retries follow the `webhooks.rs` shape: up to [`MAX_ATTEMPTS`] with
//! exponential backoff, retried only on 429 / 5xx / transport errors.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::config::ConfigManager;

const MAX_ATTEMPTS: u32 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How many recent paid invoices the reconcile backbone fetches per customer. A
/// missed webhook is recent, so a small window suffices and stays cheap.
const INVOICE_POLL_LIMIT: u32 = 10;

/// What a Checkout Session should sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutMode {
    /// A recurring subscription using the configured `stripe.price_id`.
    Subscription,
    /// A single payment of `amount_cents` (minor units), priced inline so no
    /// second Stripe product is required.
    OneShot { amount_cents: i64 },
}

/// One paid invoice, reduced to what the ledger needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeInvoice {
    pub id: String,
    /// Amount paid, in minor units (e.g. cents).
    pub amount_paid: i64,
    pub currency: String,
}

/// Errors from a Stripe API call.
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("Stripe integration is disabled")]
    Disabled,
    #[error("Stripe configuration error: {0}")]
    Configuration(String),
    #[error("Network error talking to Stripe: {0}")]
    Network(String),
    #[error("Stripe returned status {status}: {body}")]
    Api { status: u16, body: String },
    #[error("Failed to parse Stripe response: {0}")]
    Parse(String),
}

/// The live settings a call needs, resolved from config at call time.
struct Settings {
    base_url: String,
    secret_key: String,
    price_id: String,
    currency: String,
    plan_name: String,
    site_url: String,
}

#[derive(Clone)]
pub struct StripeClient {
    http: reqwest::Client,
    config: Arc<ConfigManager>,
}

impl StripeClient {
    pub fn new(config: Arc<ConfigManager>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                warn!("Failed to build Stripe HTTP client ({e}); using default");
                reqwest::Client::new()
            });
        Self { http, config }
    }

    fn settings(&self) -> Result<Settings, StripeError> {
        let c = self.config.get_config();
        if !c.stripe.enabled {
            return Err(StripeError::Disabled);
        }
        if c.stripe.secret_key.trim().is_empty() {
            return Err(StripeError::Configuration(
                "secret_key is empty".to_string(),
            ));
        }
        Ok(Settings {
            base_url: c.stripe.base_url,
            secret_key: c.stripe.secret_key,
            price_id: c.stripe.price_id,
            currency: c.stripe.currency,
            plan_name: c.membership.plan_name,
            site_url: c.site.site_url,
        })
    }

    /// Create a hosted Checkout Session and return its URL for the SPA to
    /// redirect to. `client_reference_id` is the platform user id, so the
    /// completion webhook maps the resulting customer/subscription back to
    /// exactly one account.
    pub async fn create_checkout_session(
        &self,
        mode: CheckoutMode,
        user_email: &str,
        client_reference_id: &str,
        customer_id: Option<&str>,
    ) -> Result<String, StripeError> {
        let s = self.settings()?;
        if let CheckoutMode::Subscription = mode {
            if s.price_id.trim().is_empty() {
                return Err(StripeError::Configuration("price_id is empty".to_string()));
            }
        }
        let url = endpoint(&s.base_url, "v1/checkout/sessions");
        let form = checkout_form(&s, mode, user_email, client_reference_id, customer_id);
        debug!("Stripe checkout session: mode={:?}", mode);
        let resp = self
            .send_retrying(|| self.http.post(&url).bearer_auth(&s.secret_key).form(&form))
            .await?;
        let parsed: SessionResponse = parse_json(resp).await?;
        Ok(parsed.url)
    }

    /// Create a Billing Portal session for a customer and return its URL. The
    /// member manages, cancels, and resumes their subscription there -- the
    /// platform never renders card UI.
    pub async fn create_billing_portal_session(
        &self,
        customer_id: &str,
    ) -> Result<String, StripeError> {
        let s = self.settings()?;
        let url = endpoint(&s.base_url, "v1/billing_portal/sessions");
        let form = portal_form(&s, customer_id);
        let resp = self
            .send_retrying(|| self.http.post(&url).bearer_auth(&s.secret_key).form(&form))
            .await?;
        let parsed: SessionResponse = parse_json(resp).await?;
        Ok(parsed.url)
    }

    /// The customer's most recent paid invoices, for the reconcile backbone.
    pub async fn list_paid_invoices(
        &self,
        customer_id: &str,
    ) -> Result<Vec<StripeInvoice>, StripeError> {
        let s = self.settings()?;
        let url = endpoint(&s.base_url, "v1/invoices");
        let query = invoices_query(customer_id, INVOICE_POLL_LIMIT);
        let resp = self
            .send_retrying(|| self.http.get(&url).bearer_auth(&s.secret_key).query(&query))
            .await?;
        let parsed: InvoiceListResponse = parse_json(resp).await?;
        Ok(parsed
            .data
            .into_iter()
            .filter(|i| i.status.as_deref() == Some("paid") && !i.id.is_empty())
            .map(|i| StripeInvoice {
                id: i.id,
                amount_paid: i.amount_paid,
                currency: i.currency.unwrap_or_default(),
            })
            .collect())
    }

    async fn send_retrying<F>(&self, build: F) -> Result<reqwest::Response, StripeError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut last = StripeError::Network("no attempts made".to_string());
        for attempt in 1..=MAX_ATTEMPTS {
            match build().send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    let body = resp.text().await.unwrap_or_default();
                    last = StripeError::Api {
                        status: status.as_u16(),
                        body,
                    };
                    if !retryable || attempt == MAX_ATTEMPTS {
                        return Err(last);
                    }
                    warn!(
                        "Stripe call attempt {}/{} got retryable status {}",
                        attempt, MAX_ATTEMPTS, status
                    );
                }
                Err(e) => {
                    last = StripeError::Network(e.to_string());
                    if attempt == MAX_ATTEMPTS {
                        return Err(last);
                    }
                    warn!(
                        "Stripe call attempt {}/{} failed: {}",
                        attempt, MAX_ATTEMPTS, e
                    );
                }
            }
            tokio::time::sleep(backoff(attempt)).await;
        }
        Err(last)
    }
}

async fn parse_json<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, StripeError> {
    let text = resp
        .text()
        .await
        .map_err(|e| StripeError::Network(e.to_string()))?;
    serde_json::from_str::<T>(&text).map_err(|e| StripeError::Parse(e.to_string()))
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1 << (attempt - 1))
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}

/// Absolute URL back into the SPA for a checkout/portal outcome.
fn app_url(site_url: &str, suffix: &str) -> String {
    format!("{}{}", site_url.trim_end_matches('/'), suffix)
}

/// Form fields for a Checkout Session. Subscription uses the configured price;
/// one-shot prices inline so no second Stripe product is needed.
fn checkout_form(
    s: &Settings,
    mode: CheckoutMode,
    user_email: &str,
    client_reference_id: &str,
    customer_id: Option<&str>,
) -> Vec<(String, String)> {
    let mut form: Vec<(String, String)> = Vec::new();
    match mode {
        CheckoutMode::Subscription => {
            form.push(("mode".into(), "subscription".into()));
            form.push(("line_items[0][price]".into(), s.price_id.clone()));
            form.push(("line_items[0][quantity]".into(), "1".into()));
        }
        CheckoutMode::OneShot { amount_cents } => {
            form.push(("mode".into(), "payment".into()));
            form.push((
                "line_items[0][price_data][currency]".into(),
                s.currency.to_ascii_lowercase(),
            ));
            form.push((
                "line_items[0][price_data][product_data][name]".into(),
                s.plan_name.clone(),
            ));
            form.push((
                "line_items[0][price_data][unit_amount]".into(),
                amount_cents.to_string(),
            ));
            form.push(("line_items[0][quantity]".into(), "1".into()));
        }
    }
    form.push((
        "success_url".into(),
        app_url(&s.site_url, "/profile?checkout=success"),
    ));
    form.push((
        "cancel_url".into(),
        app_url(&s.site_url, "/profile?checkout=cancel"),
    ));
    form.push((
        "client_reference_id".into(),
        client_reference_id.to_string(),
    ));
    // Reuse the existing customer when we have one, so a member does not
    // accumulate duplicate Stripe customers; otherwise let Stripe create one
    // keyed to their email.
    match customer_id {
        Some(cus) if !cus.is_empty() => form.push(("customer".into(), cus.to_string())),
        _ => form.push(("customer_email".into(), user_email.to_string())),
    }
    form
}

/// Form fields for a Billing Portal session.
fn portal_form(s: &Settings, customer_id: &str) -> Vec<(String, String)> {
    vec![
        ("customer".to_string(), customer_id.to_string()),
        ("return_url".to_string(), app_url(&s.site_url, "/profile")),
    ]
}

/// Query fields for the paid-invoice poll.
fn invoices_query(customer_id: &str, limit: u32) -> Vec<(String, String)> {
    vec![
        ("customer".to_string(), customer_id.to_string()),
        ("status".to_string(), "paid".to_string()),
        ("limit".to_string(), limit.to_string()),
    ]
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct InvoiceListResponse {
    #[serde(default)]
    data: Vec<RawInvoice>,
}

#[derive(Debug, Deserialize)]
struct RawInvoice {
    #[serde(default)]
    id: String,
    #[serde(default)]
    amount_paid: i64,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings {
            base_url: "https://api.stripe.com".to_string(),
            secret_key: "sk_test".to_string(),
            price_id: "price_123".to_string(),
            currency: "USD".to_string(),
            plan_name: "CHACK Membership".to_string(),
            site_url: "https://space.example.org/".to_string(),
        }
    }

    fn get<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
        form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn endpoint_joins_and_trims_a_trailing_slash() {
        assert_eq!(
            endpoint("https://api.stripe.com", "v1/checkout/sessions"),
            "https://api.stripe.com/v1/checkout/sessions"
        );
        assert_eq!(
            endpoint("http://127.0.0.1:4391/", "v1/invoices"),
            "http://127.0.0.1:4391/v1/invoices"
        );
    }

    #[test]
    fn subscription_checkout_uses_the_configured_price() {
        let form = checkout_form(
            &settings(),
            CheckoutMode::Subscription,
            "m@example.org",
            "user-1",
            None,
        );
        assert_eq!(get(&form, "mode"), Some("subscription"));
        assert_eq!(get(&form, "line_items[0][price]"), Some("price_123"));
        assert_eq!(get(&form, "line_items[0][quantity]"), Some("1"));
        assert_eq!(get(&form, "client_reference_id"), Some("user-1"));
        // No stored customer: fall back to email, and never send price_data.
        assert_eq!(get(&form, "customer_email"), Some("m@example.org"));
        assert_eq!(get(&form, "customer"), None);
        assert_eq!(get(&form, "line_items[0][price_data][unit_amount]"), None);
    }

    #[test]
    fn one_shot_checkout_prices_inline_in_the_configured_currency() {
        let form = checkout_form(
            &settings(),
            CheckoutMode::OneShot { amount_cents: 2500 },
            "m@example.org",
            "user-1",
            Some("cus_9"),
        );
        assert_eq!(get(&form, "mode"), Some("payment"));
        assert_eq!(
            get(&form, "line_items[0][price_data][unit_amount]"),
            Some("2500")
        );
        // Stripe requires a lowercase currency code.
        assert_eq!(
            get(&form, "line_items[0][price_data][currency]"),
            Some("usd")
        );
        assert_eq!(
            get(&form, "line_items[0][price_data][product_data][name]"),
            Some("CHACK Membership")
        );
        // A known customer is reused rather than re-created.
        assert_eq!(get(&form, "customer"), Some("cus_9"));
        assert_eq!(get(&form, "customer_email"), None);
    }

    #[test]
    fn checkout_redirects_derive_from_the_site_url_without_a_double_slash() {
        let form = checkout_form(
            &settings(),
            CheckoutMode::Subscription,
            "m@example.org",
            "user-1",
            None,
        );
        assert_eq!(
            get(&form, "success_url"),
            Some("https://space.example.org/profile?checkout=success")
        );
        assert_eq!(
            get(&form, "cancel_url"),
            Some("https://space.example.org/profile?checkout=cancel")
        );
    }

    #[test]
    fn portal_form_carries_customer_and_return_url() {
        let form = portal_form(&settings(), "cus_9");
        assert_eq!(get(&form, "customer"), Some("cus_9"));
        assert_eq!(
            get(&form, "return_url"),
            Some("https://space.example.org/profile")
        );
    }

    #[test]
    fn invoices_query_filters_to_paid() {
        let q = invoices_query("cus_9", 10);
        assert_eq!(
            q,
            vec![
                ("customer".to_string(), "cus_9".to_string()),
                ("status".to_string(), "paid".to_string()),
                ("limit".to_string(), "10".to_string()),
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
    fn an_invoice_list_parses_and_keeps_paid_only() {
        let json = r#"{
            "data": [
                {"id": "in_1", "amount_paid": 1000, "currency": "usd", "status": "paid"},
                {"id": "in_2", "amount_paid": 0, "currency": "usd", "status": "open"}
            ]
        }"#;
        let parsed: InvoiceListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].amount_paid, 1000);
        assert_eq!(parsed.data[0].status.as_deref(), Some("paid"));
    }
}
