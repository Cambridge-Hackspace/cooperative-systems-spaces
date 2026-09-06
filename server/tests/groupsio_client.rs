//! The Groups.io client over a real HTTP conversation.
//!
//! The pure builders are unit-tested in `groupsio.rs`; this is the only place
//! that proves the client actually *speaks* to a server -- that `.form()` puts
//! the group and the newline-joined addresses on the wire, that the bearer
//! token is attached, and that `get_members` follows pagination to the end
//! rather than returning only the first page.
//!
//! The destination is an in-process axum sink bound to an ephemeral port,
//! recording what it received, in the spirit of `css-webhook-recvr`. Assertions
//! read what arrived at the sink, not what the client said it sent.
//!
//! Two oracles for the safety property that matters: a disabled client both
//! returns `Disabled` *and* leaves the sink's request count untouched -- proving
//! no request left the process, which the error alone cannot.

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Form, Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use css_server::config::{AppConfig, ConfigManager};
use css_server::groupsio::{GroupsioClient, GroupsioError};
use serde_json::json;
use std::collections::HashMap;

/// What the fake Groups.io recorded.
#[derive(Default)]
struct Sink {
    directadd_bodies: Vec<HashMap<String, String>>,
    remove_bodies: Vec<HashMap<String, String>>,
    getmembers_calls: usize,
    total_requests: usize,
}

type SharedSink = Arc<Mutex<Sink>>;

async fn directadd(
    State(sink): State<SharedSink>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let mut s = sink.lock().unwrap();
    s.total_requests += 1;
    s.directadd_bodies.push(form);
    Json(json!({ "object": "member_list", "results": [] }))
}

async fn bulkremove(
    State(sink): State<SharedSink>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let mut s = sink.lock().unwrap();
    s.total_requests += 1;
    s.remove_bodies.push(form);
    Json(json!({ "object": "member_list", "results": [] }))
}

/// Two-page members fixture keyed on the page token, so the client's pagination
/// loop is genuinely exercised rather than short-circuited on page one.
async fn getmembers(
    State(sink): State<SharedSink>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    {
        let mut s = sink.lock().unwrap();
        s.total_requests += 1;
        s.getmembers_calls += 1;
    }
    match q.get("page_token").map(String::as_str) {
        None => Json(json!({
            "data": [{ "email": "a@example.org", "status": "normal" }],
            "has_more": true,
            "next_page_token": 2
        })),
        Some("2") => Json(json!({
            "data": [{ "email": "b@example.org", "status": "normal" }],
            "has_more": false,
            "next_page_token": 0
        })),
        Some(other) => panic!("unexpected page_token {other}"),
    }
}

async fn getpastmembers(State(sink): State<SharedSink>) -> Json<serde_json::Value> {
    sink.lock().unwrap().total_requests += 1;
    Json(json!({ "data": [], "has_more": false, "next_page_token": 0 }))
}

/// Start the sink on an ephemeral port; return its base URL and the shared state.
async fn start_sink() -> (String, SharedSink) {
    let sink: SharedSink = Arc::new(Mutex::new(Sink::default()));
    let app = Router::new()
        .route("/directadd", post(directadd))
        .route("/bulkremovemembers", post(bulkremove))
        .route("/getmembers", get(getmembers))
        .route("/getpastmembers", get(getpastmembers))
        .with_state(sink.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), sink)
}

/// A client pointed at `base_url`, enabled unless `enabled` says otherwise.
fn client(base_url: &str, enabled: bool) -> GroupsioClient {
    let mut cfg = AppConfig::default();
    cfg.groupsio.enabled = enabled;
    cfg.groupsio.api_key = "test-key".to_string();
    cfg.groupsio.group_id = "g1".to_string();
    cfg.groupsio.base_url = base_url.to_string();
    GroupsioClient::new(Arc::new(ConfigManager::new(cfg, None)))
}

#[tokio::test]
async fn direct_add_puts_the_group_and_newline_joined_addresses_on_the_wire() {
    let (base, sink) = start_sink().await;
    let c = client(&base, true);

    c.direct_add(&["a@example.org".to_string(), "b@example.org".to_string()])
        .await
        .expect("direct_add succeeds against the sink");

    let s = sink.lock().unwrap();
    assert_eq!(s.directadd_bodies.len(), 1);
    let body = &s.directadd_bodies[0];
    assert_eq!(body.get("group_id").map(String::as_str), Some("g1"));
    assert_eq!(
        body.get("emails").map(String::as_str),
        Some("a@example.org\nb@example.org")
    );
}

#[tokio::test]
async fn remove_members_posts_to_bulk_remove() {
    let (base, sink) = start_sink().await;
    let c = client(&base, true);

    c.remove_members(&["gone@example.org".to_string()])
        .await
        .expect("remove succeeds against the sink");

    let s = sink.lock().unwrap();
    assert_eq!(s.remove_bodies.len(), 1);
    assert_eq!(
        s.remove_bodies[0].get("emails").map(String::as_str),
        Some("gone@example.org")
    );
}

#[tokio::test]
async fn get_members_follows_pagination_to_the_end() {
    let (base, sink) = start_sink().await;
    let c = client(&base, true);

    let members = c.get_members().await.expect("get_members succeeds");
    let emails: Vec<String> = members.into_iter().map(|m| m.email).collect();

    assert_eq!(emails, vec!["a@example.org", "b@example.org"]);
    // Two pages means two calls: pagination genuinely looped.
    assert_eq!(sink.lock().unwrap().getmembers_calls, 2);
}

#[tokio::test]
async fn a_disabled_client_makes_no_request_at_all() {
    let (base, sink) = start_sink().await;
    let c = client(&base, false);

    // Precondition asserted before the outcome: nothing has been sent yet.
    assert_eq!(sink.lock().unwrap().total_requests, 0);

    let err = c
        .direct_add(&["a@example.org".to_string()])
        .await
        .expect_err("a disabled client must refuse");
    assert!(matches!(err, GroupsioError::Disabled));

    // The second oracle: the sink confirms no request arrived, which the error
    // alone cannot establish.
    assert_eq!(
        sink.lock().unwrap().total_requests,
        0,
        "a disabled client must not touch the network"
    );
}
