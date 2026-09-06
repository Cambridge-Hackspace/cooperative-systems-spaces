//! `css-groupsio-sink` -- a stateful fake of the Groups.io member API.
//!
//! The stack tier's mailing-list destination. It keeps an in-memory group
//! roster and a past-members list, and answers the four endpoints the server's
//! `GroupsioClient` calls: `directadd`, `bulkremovemembers`, `getmembers`,
//! `getpastmembers`. Same reasoning as `css-webhook-recvr` and `css-smtp-sink`:
//! an in-repo binary built alongside the server needs no container image.
//!
//! Membership is case-insensitive, as Groups.io's is, so a mixed-case address
//! the server sends does not create a second entry. A remove moves an address
//! to past-members (where a real unsubscribe lands), so the reconciler's
//! "learned they left via an email link" path has something to observe.
//!
//! Beyond the API it exposes a small control surface, namespaced under
//! `/_control/`, for the driver to seed a starting roster (e.g. a stranger the
//! platform did not add, or a protected owner) and to read the roster back:
//!
//!   POST /_control/seed    {"members": ["a@x", ...], "past": ["b@x", ...]}
//!   GET  /_control/roster  -> {"members": [...], "past": [...]}
//!   POST /_control/reset   -> empties both lists
//!
//!   css-groupsio-sink --bind 127.0.0.1:4400

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Form, State},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use clap::Parser;
use serde::Deserialize;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(
    name = "css-groupsio-sink",
    about = "Stateful fake of the Groups.io member API"
)]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "GROUPSIO_SINK_BIND", default_value = "127.0.0.1:4400")]
    bind: String,
}

#[derive(Default)]
struct Group {
    members: HashSet<String>,
    past: HashSet<String>,
}

#[derive(Clone)]
struct AppState {
    group: Arc<Mutex<Group>>,
}

fn norm(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Split the `emails` field, which `directadd` / `bulkremovemembers` send as a
/// newline-separated list.
fn split_emails(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r', ','])
        .map(norm)
        .filter(|e| !e.is_empty())
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let state = AppState {
        group: Arc::new(Mutex::new(Group::default())),
    };

    let app = Router::new()
        .route("/directadd", post(directadd))
        .route("/bulkremovemembers", post(bulkremove))
        .route("/getmembers", get(getmembers))
        .route("/getpastmembers", get(getpastmembers))
        .route("/_control/seed", post(control_seed))
        .route("/_control/roster", get(control_roster))
        .route("/_control/reset", post(control_reset))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    println!("css-groupsio-sink listening on {}", args.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn directadd(
    State(st): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let emails = form
        .get("emails")
        .map(|s| split_emails(s))
        .unwrap_or_default();
    let mut g = st.group.lock().unwrap();
    for e in &emails {
        g.past.remove(e);
        g.members.insert(e.clone());
    }
    Json(json!({ "object": "member_list", "results": [] }))
}

async fn bulkremove(
    State(st): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let emails = form
        .get("emails")
        .map(|s| split_emails(s))
        .unwrap_or_default();
    let mut g = st.group.lock().unwrap();
    for e in &emails {
        if g.members.remove(e) {
            g.past.insert(e.clone());
        }
    }
    Json(json!({ "object": "member_list", "results": [] }))
}

/// A member-list page. The fake returns everything on one page (`has_more`
/// false), which the client's pagination loop handles as the terminal page.
fn member_page(emails: &HashSet<String>) -> serde_json::Value {
    let data: Vec<serde_json::Value> = emails
        .iter()
        .map(|e| json!({ "email": e, "status": "normal" }))
        .collect();
    json!({ "object": "member_list", "data": data, "has_more": false, "next_page_token": 0 })
}

async fn getmembers(State(st): State<AppState>) -> impl IntoResponse {
    let g = st.group.lock().unwrap();
    Json(member_page(&g.members))
}

async fn getpastmembers(State(st): State<AppState>) -> impl IntoResponse {
    let g = st.group.lock().unwrap();
    Json(member_page(&g.past))
}

#[derive(Deserialize, Default)]
struct SeedBody {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    past: Vec<String>,
}

async fn control_seed(State(st): State<AppState>, Json(body): Json<SeedBody>) -> impl IntoResponse {
    let mut g = st.group.lock().unwrap();
    for e in body.members {
        g.members.insert(norm(&e));
    }
    for e in body.past {
        g.past.insert(norm(&e));
    }
    Json(json!({ "ok": true }))
}

async fn control_roster(State(st): State<AppState>) -> impl IntoResponse {
    let g = st.group.lock().unwrap();
    let mut members: Vec<String> = g.members.iter().cloned().collect();
    let mut past: Vec<String> = g.past.iter().cloned().collect();
    members.sort();
    past.sort();
    Json(json!({ "members": members, "past": past }))
}

async fn control_reset(State(st): State<AppState>) -> impl IntoResponse {
    let mut g = st.group.lock().unwrap();
    g.members.clear();
    g.past.clear();
    Json(json!({ "ok": true }))
}
