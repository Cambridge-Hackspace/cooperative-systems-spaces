//! `css-webhook-recvr` — a tiny test sink for the webhook dispatcher.
//!
//! Accepts any request on any path and pretty-prints the method, path,
//! headers, and body to stdout. If `--secret` is given, it recomputes the
//! HMAC-SHA256 signature over the raw body and reports whether the
//! `X-Webhook-Signature` header matches.
//!
//! Examples:
//!   css-webhook-recvr                      # listen on 127.0.0.1:4398
//!   css-webhook-recvr --bind 0.0.0.0:4398  # listen on all interfaces
//!   css-webhook-recvr --secret <signing_secret>
//!
//! Point a webhook's URL at e.g. http://127.0.0.1:4398/hook and fire its Test.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Parser, Debug)]
#[command(
    name = "css-webhook-recvr",
    about = "Test sink that prints received webhooks"
)]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "WEBHOOK_RECVR_BIND", default_value = "127.0.0.1:4398")]
    bind: String,

    /// Webhook signing secret. When set, verifies the X-Webhook-Signature header.
    #[arg(long, env = "WEBHOOK_RECVR_SECRET")]
    secret: Option<String>,
}

#[derive(Clone)]
struct AppState {
    secret: Option<Arc<String>>,
    counter: Arc<AtomicU64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let state = AppState {
        secret: args.secret.map(Arc::new),
        counter: Arc::new(AtomicU64::new(0)),
    };

    let app = Router::new()
        .route("/", get(root_index))
        .fallback(handler)
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    println!("╭───────────────────────────────────────────────╮");
    println!("│  css-webhook-recvr listening on {:<14}│", args.bind);
    println!(
        "│  signature check: {:<28}│",
        if state.secret.is_some() {
            "enabled"
        } else {
            "disabled (no --secret)"
        }
    );
    println!("╰───────────────────────────────────────────────╯");

    axum::serve(listener, app).await?;
    Ok(())
}

/// GET / — a human-readable listing of what this receiver accepts.
async fn root_index() -> impl IntoResponse {
    (
        StatusCode::OK,
        "css-webhook-recvr — test webhook sink\n\
         \n\
         Routes:\n\
           POST /hook   receive a webhook and print it to stdout\n\
           ANY  /*      any other path is also received and printed\n\
           GET  /       this listing\n\
         \n\
         Point a webhook URL at http://<host>:4398/hook\n",
    )
}

async fn handler(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let n = state.counter.fetch_add(1, Ordering::Relaxed) + 1;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");

    println!("\n═══ webhook #{n}  {now} ════════════════════════════");
    println!("{method} {uri}");

    // Headers, sorted for readability.
    println!("── headers ──");
    let mut names: Vec<_> = headers.keys().collect();
    names.sort_by_key(|k| k.as_str());
    for name in names {
        for value in headers.get_all(name) {
            let v = value.to_str().unwrap_or("<non-utf8>");
            println!("  {}: {}", name, v);
        }
    }

    // Signature verification (if a secret was supplied).
    if let Some(secret) = &state.secret {
        match headers
            .get("x-webhook-signature")
            .and_then(|v| v.to_str().ok())
        {
            Some(provided) => {
                let expected = format!("sha256={}", sign(secret, &body));
                let ok = provided == expected;
                println!("── signature ──");
                println!("  header:   {provided}");
                println!("  expected: {expected}");
                println!("  result:   {}", if ok { "✓ VALID" } else { "✗ INVALID" });
            }
            None => println!("── signature ──\n  (no X-Webhook-Signature header present)"),
        }
    }

    // Body — pretty-print JSON if possible, otherwise raw.
    println!("── body ({} bytes) ──", body.len());
    match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(json) => println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .unwrap_or_else(|_| String::from_utf8_lossy(&body).into_owned())
        ),
        Err(_) => println!("{}", String::from_utf8_lossy(&body)),
    }
    println!("═══════════════════════════════════════════════════════");

    (StatusCode::OK, "ok\n")
}

/// Hex-encoded HMAC-SHA256 of `body` keyed by `secret`.
fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}
