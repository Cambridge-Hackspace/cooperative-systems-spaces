//! `css-stripe-sink` -- a fake of the small slice of the Stripe API the
//! membership module calls.
//!
//! The stack tier's Stripe stand-in. It answers the three endpoints the server's
//! `StripeClient` uses -- create a Checkout Session, create a Billing Portal
//! session, and list a customer's paid invoices -- and keeps an in-memory set of
//! paid invoices per customer so the reconcile-poll backbone has something to
//! read. Same reasoning as `css-groupsio-sink`: an in-repo binary built
//! alongside the server needs no container image.
//!
//! It does NOT verify bearer auth (it is a fake) and it does NOT mint webhooks:
//! the driver constructs and HMAC-signs Stripe events itself and posts them to
//! the server's `/api/stripe/webhook`. The sink's job is only the outbound calls
//! the server makes.
//!
//! Control surface, namespaced under `/_control/`:
//!
//!   POST /_control/paid-invoice  {"customer": "...", "id": "in_1",
//!                                 "amount_paid": 1000, "currency": "usd"}
//!       -- add a paid invoice to a customer's list, WITHOUT a webhook, so the
//!          driver can prove the poll catches a payment whose webhook was missed.
//!   POST /_control/reset -> forget all invoices
//!
//!   css-stripe-sink --bind 127.0.0.1:4391

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use clap::Parser;
use serde::Deserialize;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(
    name = "css-stripe-sink",
    about = "Fake of the Stripe API slice the server calls"
)]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "STRIPE_SINK_BIND", default_value = "127.0.0.1:4391")]
    bind: String,
}

#[derive(Clone)]
struct Invoice {
    id: String,
    amount_paid: i64,
    currency: String,
}

#[derive(Default)]
struct Store {
    /// customer id -> its paid invoices.
    invoices: HashMap<String, Vec<Invoice>>,
}

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<Store>>,
    seq: Arc<AtomicU64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let state = AppState {
        store: Arc::new(Mutex::new(Store::default())),
        seq: Arc::new(AtomicU64::new(1)),
    };

    let app = Router::new()
        .route("/v1/checkout/sessions", post(create_checkout))
        .route("/v1/billing_portal/sessions", post(create_portal))
        .route("/v1/invoices", get(list_invoices))
        .route("/_control/paid-invoice", post(control_paid_invoice))
        .route("/_control/reset", post(control_reset))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    println!("css-stripe-sink listening on {}", args.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Return a hosted-checkout URL. The server reads only `url`; the driver invents
/// its own customer/subscription ids for the webhook it signs, so the exact ids
/// here do not need to match anything.
async fn create_checkout(State(st): State<AppState>) -> impl IntoResponse {
    let n = st.seq.fetch_add(1, Ordering::SeqCst);
    let id = format!("cs_test_{n}");
    Json(json!({
        "id": id,
        "object": "checkout.session",
        "url": format!("https://stripe.test/checkout/{id}"),
        "customer": format!("cus_test_{n}"),
    }))
}

async fn create_portal(State(st): State<AppState>) -> impl IntoResponse {
    let n = st.seq.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "id": format!("bps_test_{n}"),
        "object": "billing_portal.session",
        "url": format!("https://stripe.test/portal/{n}"),
    }))
}

/// GET /v1/invoices?customer=&status=paid&limit= -- the paid invoices for a
/// customer. Filtered to the `customer` query param (Stripe requires it here).
async fn list_invoices(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let customer = q.get("customer").cloned().unwrap_or_default();
    let store = st.store.lock().unwrap();
    let data: Vec<serde_json::Value> = store
        .invoices
        .get(&customer)
        .map(|invs| {
            invs.iter()
                .map(|i| {
                    json!({
                        "id": i.id,
                        "object": "invoice",
                        "amount_paid": i.amount_paid,
                        "currency": i.currency,
                        "status": "paid",
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Json(json!({ "object": "list", "data": data, "has_more": false }))
}

#[derive(Deserialize)]
struct PaidInvoiceBody {
    customer: String,
    id: String,
    amount_paid: i64,
    #[serde(default = "default_currency")]
    currency: String,
}

fn default_currency() -> String {
    "usd".to_string()
}

async fn control_paid_invoice(
    State(st): State<AppState>,
    Json(body): Json<PaidInvoiceBody>,
) -> impl IntoResponse {
    let mut store = st.store.lock().unwrap();
    store
        .invoices
        .entry(body.customer.clone())
        .or_default()
        .push(Invoice {
            id: body.id,
            amount_paid: body.amount_paid,
            currency: body.currency,
        });
    Json(json!({ "ok": true }))
}

async fn control_reset(State(st): State<AppState>) -> impl IntoResponse {
    let mut store = st.store.lock().unwrap();
    store.invoices.clear();
    Json(json!({ "ok": true }))
}
