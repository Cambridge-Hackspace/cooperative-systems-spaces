//! Tier 4, the offline half: what the API does with a request that carries no
//! usable credential.
//!
//! Runs in-process over the real router with a non-connecting database, so it
//! needs no PostgreSQL and no containers. That covers the entire
//! request-rejection surface, because `AuthUser::from_request_parts` checks the
//! header, the `Bearer` prefix and the JWT signature *before* its single
//! `find_user_by_id` — every negative case short-circuits before the database.
//!
//! What it deliberately does **not** cover, and why: anything needing a *valid*
//! user. Role gating (`AdminUser`, `StaffUser`) delegates to `AuthUser`, which
//! loads the user from the database, so a 403-for-insufficient-role cannot be
//! reached without one. Those rows belong to the container tier and are not
//! silently folded in here.
//!
//! The rule that keeps this honest: **500 means the request reached the dead
//! pool.** It is distinct in status and body from every legitimate rejection.
//! Assertions are exact; an unexpected 500 is a failure of the test's premise,
//! not a result. `the_database_really_is_unreachable` below is the liveness
//! meta-test — if somebody ever wires a real database into this fixture, that
//! test fails loudly rather than letting every negative result here quietly
//! start meaning something else.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use css_server::{api, test_support};
use tower::ServiceExt;

/// The real API router, over the real `AppState`, with a dead pool behind it.
async fn router() -> axum::Router {
    // Nested under `/api` as main.rs mounts it, so these paths are the ones a
    // client actually requests.
    axum::Router::new()
        .nest("/api", api::api_routes())
        .with_state(test_support::app_state().await)
}

async fn send(req: Request<Body>) -> (StatusCode, String) {
    let response = router()
        .await
        .oneshot(req)
        .await
        .expect("the router is infallible");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("well-formed request")
}

fn get_with_auth(path: &str, value: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("authorization", value)
        .body(Body::empty())
        .expect("well-formed request")
}

/// A route behind `AuthUser`, chosen because it is the cheapest guarded thing
/// in the API and takes no path parameters.
const GUARDED: &str = "/api/auth/me";

#[tokio::test]
async fn a_guarded_route_refuses_a_request_with_no_authorization_header() {
    let (status, body) = send(get(GUARDED)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "no credential must be 401, not 400: the request is well-formed, it is \
         unauthenticated. Body was: {body}"
    );
}

#[tokio::test]
async fn a_guarded_route_refuses_every_unusable_credential() {
    // Each of these short-circuits at a different point in
    // `AuthUser::from_request_parts`, and all of them before the database.
    let cases: &[(&str, &str)] = &[
        ("not a bearer token at all", "Basic dXNlcjpwYXNz"),
        ("the Bearer prefix with nothing after it", "Bearer "),
        ("a token that is not a JWT", "Bearer not-a-jwt"),
        (
            "a JWT with three segments but a bad signature",
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.wrong",
        ),
    ];

    for (what, header) in cases {
        let (status, body) = send(get_with_auth(GUARDED, header)).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{what} must be refused with 401. Body was: {body}"
        );
    }
}

#[tokio::test]
async fn a_non_utf8_authorization_header_is_refused_rather_than_panicking() {
    // `to_str()` on the header fails here. Worth its own case because a header
    // is attacker-controlled bytes and the failure mode of getting this wrong
    // is a panic in an extractor, which axum turns into a 500.
    let req = Request::builder()
        .uri(GUARDED)
        .header("authorization", &b"Bearer \xff\xfe"[..])
        .body(Body::empty())
        .expect("a non-UTF-8 header value is a legal HTTP header");
    let (status, _) = send(req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_unguarded_route_is_still_reachable() {
    // Guards the guard. If every route 404'd — a mistyped path, a router that
    // failed to build — the assertions above would pass for the wrong reason,
    // because a 404 is not a 401 but neither is it evidence of authentication.
    let (status, _) = send(get("/api/config/public")).await;
    assert_ne!(
        status,
        StatusCode::NOT_FOUND,
        "the API router is not mounting routes; every assertion in this file is \
         meaningless until that is fixed"
    );
}

#[tokio::test]
async fn a_guarded_route_exists_and_is_not_merely_absent() {
    // The same trap, for the one route the tests above lean on. A mistyped
    // GUARDED path would 404 uniformly and look reassuringly consistent.
    let (status, _) = send(
        Request::builder()
            .method("TRACE")
            .uri(GUARDED)
            .body(Body::empty())
            .expect("well-formed request"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::METHOD_NOT_ALLOWED,
        "{GUARDED} should exist and reject TRACE; a 404 here means the path is \
         wrong and the 401s above prove nothing"
    );
}

#[tokio::test]
async fn the_database_really_is_unreachable() {
    // The liveness meta-test, and the reason the rest of this file can be
    // trusted. `POST /auth/login` needs no credential but does reach
    // `find_user_by_username`, so with a dead pool it must be a 500.
    //
    // If someone later points this fixture at a real database, this test fails
    // and says so — rather than every negative assertion above quietly starting
    // to mean "the database said no" instead of "the request was refused before
    // the database was consulted".
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"username_or_email":"nobody","password":"x"}"#,
        ))
        .expect("well-formed request");

    let (status, _) = send(req).await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "expected the dead pool to surface as 500. If this is now a 401, the \
         fixture has a working database behind it and this whole file is \
         testing something other than what it claims."
    );
}

#[tokio::test]
async fn a_router_nested_at_slash_answers_without_the_trailing_slash() {
    // Found by the guard above rather than by reading the docs.
    //
    // `toolguard_routes()` registers `.route("/", get(api_status))` and is
    // nested under `/toolguard`. In axum 0.8 that resolves to `/api/toolguard`
    // exactly — `/api/toolguard/` is a different path and 404s.
    //
    // This is asserted rather than assumed because a client that writes the
    // trailing slash gets a 404 that looks like a deployment problem.
    // `toolpass-client`'s status command was doing exactly that.
    let (bare, _) = send(get("/api/toolguard")).await;
    assert_ne!(
        bare,
        StatusCode::NOT_FOUND,
        "/toolguard should be the status route"
    );

    let (trailing, _) = send(get("/api/toolguard/")).await;
    assert_eq!(
        trailing,
        StatusCode::NOT_FOUND,
        "if axum starts matching the trailing-slash form too, this test should \
         be relaxed deliberately rather than left asserting the old behaviour"
    );
}
