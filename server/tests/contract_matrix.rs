//! Tier 4: every route × every credential state that carries no valid user.
//!
//! 164 routes × 7 credential states. Runs in-process over the real router with
//! a non-connecting database, so it needs no PostgreSQL and no containers.
//!
//! The expectation is deliberately **one line**, not a thousand hand-written
//! cells: *no route behind a guard ever answers anything but 401 without a
//! valid credential.* That is a strong claim precisely because it is uniform —
//! a handler that forgot its extractor answers 200 or 500 instead of 401 and
//! the whole column lights up. It is exactly the class of defect that
//! `/api/toolguard/tool-on` was: reachable by URL with no credential at all,
//! sitting beside four siblings that authenticated correctly.
//!
//! Public routes are asserted for the complementary property — that they do
//! *not* demand a credential — but not for an exact status, because most of
//! them reach the database and therefore 500 against this fixture. A row whose
//! honest offline answer is 500 belongs to the live-database tier and is not
//! folded in here.
//!
//! What this cannot see, stated rather than implied: role gating. `AdminUser`
//! and `StaffUser` delegate to `AuthUser`, which loads the user from the
//! database, so 403-for-insufficient-role needs a real Postgres. Those rows are
//! the live-database tier's, and `guarded_routes_are_not_asserted_for_role_gating`
//! below records that boundary so it cannot be quietly forgotten.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{Guard, ROUTES};
use css_server::{api, test_support, AppState};
use tower::ServiceExt;

/// Credential states that carry no valid user.
///
/// Every one of these short-circuits inside `AuthUser::from_request_parts`
/// before its single `find_user_by_id` — header presence, `to_str`, the
/// `Bearer` prefix, then a pure-HMAC JWT verify. That ordering is why the whole
/// matrix is reachable without a database, and it is asserted independently by
/// `the_database_is_never_reached_by_a_rejected_request` below.
#[derive(Clone, Copy)]
struct Cred {
    what: &'static str,
    header: Option<&'static str>,
    /// True when the credential is rejected on its *shape* — absent, empty, or
    /// not a `Bearer` scheme — rather than on its contents.
    ///
    /// The distinction matters for device-authenticated routes. `AuthUser`
    /// verifies a JWT cryptographically before it ever queries, so every
    /// credential state below short-circuits offline. `DeviceAuth` cannot:
    /// a device token is an opaque string, so validating one *is* a database
    /// lookup (`find_device_by_auth_token`), and anything that survives the
    /// shape checks reaches the dead pool and answers 500.
    ///
    /// So those rows are the live-database tier's, and
    /// `the_offline_device_surface_is_exactly_this_narrow` below pins how many
    /// they are, so the split cannot quietly widen.
    shape_only: bool,
}

const CREDS: &[Cred] = &[
    Cred {
        what: "no Authorization header",
        header: None,
        shape_only: true,
    },
    Cred {
        what: "an empty Authorization header",
        header: Some(""),
        shape_only: true,
    },
    Cred {
        what: "a non-Bearer scheme",
        header: Some("Basic dXNlcjpwYXNz"),
        shape_only: true,
    },
    Cred {
        // `Bearer ` with an empty token still *has* the scheme, so DeviceAuth
        // proceeds to look the empty string up in the database.
        what: "Bearer with nothing after it",
        header: Some("Bearer "),
        shape_only: false,
    },
    Cred {
        what: "a token that is not a JWT",
        header: Some("Bearer not-a-jwt"),
        shape_only: false,
    },
    Cred {
        what: "a JWT signed with the wrong key",
        header: Some("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.wrong"),
        shape_only: false,
    },
    Cred {
        // `alg` confusion: the server pins HS256 via `Validation::new(Algorithm::HS256)`,
        // and a token claiming a different algorithm must be refused rather
        // than accepted on its own say-so.
        what: "a JWT claiming alg=none",
        header: Some("Bearer eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiIxIn0."),
        shape_only: false,
    },
];

/// Routes whose handlers take a required `Query<..>`, with a query string that
/// satisfies it.
///
/// Needed because axum runs every `FromRequestParts` extractor before the
/// handler body, and `Query` is one of them — so a request missing `card` is
/// rejected with 400 by the query extractor and never reaches the
/// authentication check inside the handler. Without these, the matrix would be
/// asserting "a malformed request is refused", which is true and uninteresting,
/// instead of "an unauthenticated request is refused", which is the claim.
///
/// That ordering is itself a finding, pinned by
/// `toolguard_parses_parameters_before_it_authenticates` below: these three are
/// the only routes in the API that authenticate in the handler body rather than
/// through an extractor, and it is the reason they can be probed for valid
/// parameters by an unauthenticated caller.
const REQUIRED_QUERY: &[(&str, &str)] = &[
    ("/api/toolguard/tool-on", "card=x&tool_id=y"),
    ("/api/toolguard/tool-off", "card=x&tool_id=y"),
    ("/api/toolguard/tool-log", "card=x&tool_id=y&seconds=1"),
];

fn with_required_query(path: &str) -> String {
    match REQUIRED_QUERY.iter().find(|(p, _)| *p == path) {
        Some((_, q)) => format!("{path}?{q}"),
        None => path.to_string(),
    }
}

async fn state() -> AppState {
    test_support::app_state().await
}

async fn call(st: &AppState, method: &str, path: &str, auth: Option<&str>) -> StatusCode {
    let mut b = Request::builder()
        .method(method)
        .uri(with_required_query(path));
    if let Some(v) = auth {
        b = b.header("authorization", v);
    }
    // A body and a content-type on every write, so that a 415 or a 422 can
    // never be mistaken for a rejection. Axum runs `FromRequestParts`
    // extractors — which is where auth lives — strictly before the body
    // extractor, so this changes nothing about what is under test; it only
    // removes a way for the test to be wrong.
    let req = if matches!(method, "POST" | "PUT" | "PATCH") {
        b.header("content-type", "application/json")
            .body(Body::from("{}"))
    } else {
        b.body(Body::empty())
    }
    .expect("well-formed request");

    // Nested under `/api`, exactly as main.rs mounts it. Calling
    // `api_routes()` bare would serve `/auth/me` rather than `/api/auth/me`,
    // and every path in the table would 404 -- which is precisely what
    // `every_route_in_the_table_actually_exists` reported on the first run.
    // Testing the paths a client actually uses is the point.
    axum::Router::new()
        .nest("/api", api::api_routes())
        .with_state(st.clone())
        .oneshot(req)
        .await
        .expect("the router is infallible")
        .status()
}

#[tokio::test]
async fn no_guarded_route_answers_anything_but_401_without_a_credential() {
    let st = state().await;
    let mut failures: Vec<String> = Vec::new();

    for route in ROUTES.iter().filter(|r| r.is_guarded()) {
        // A device token is opaque, so DeviceAuth must query to reject it.
        // Only its shape checks are reachable without a database.
        let device_backed = matches!(route.guard(), Guard::Device | Guard::InlineAuth);

        for cred in CREDS {
            if device_backed && !cred.shape_only {
                continue;
            }
            let got = call(&st, route.method(), route.path(), cred.header).await;
            if got != StatusCode::UNAUTHORIZED {
                failures.push(format!(
                    "{} {} with {} -> {} (expected 401)",
                    route.method(),
                    route.path(),
                    cred.what,
                    got
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} guarded route/credential pairs did not refuse the request:\n{}",
        failures.len(),
        asserted_pairs(),
        failures.join("\n")
    );
}

#[tokio::test]
async fn no_public_route_demands_a_credential() {
    // The complement, and it is not redundant: a route that has quietly
    // *gained* a guard is as much a defect as one that lost it, and only this
    // direction catches it. Exact statuses are not asserted because most of
    // these reach the database.
    let st = state().await;
    let mut failures: Vec<String> = Vec::new();

    for route in ROUTES.iter().filter(|r| !r.is_guarded()) {
        let got = call(&st, route.method(), route.path(), None).await;
        if got == StatusCode::UNAUTHORIZED || got == StatusCode::FORBIDDEN {
            failures.push(format!(
                "{} {} -> {} but is declared Public",
                route.method(),
                route.path(),
                got
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
async fn every_route_in_the_table_actually_exists() {
    // The single most likely way for this file to become worthless: a mistyped
    // path 404s uniformly, and 404 is not 401, so the matrix above would report
    // a wall of failures — or worse, if the expectation were ever loosened to
    // "not 2xx", a wall of passes over routes that do not exist.
    //
    // TRACE is registered by nothing, so a route that exists answers 405 and a
    // route that does not answers 404. That distinguishes them without needing
    // a credential.
    let st = state().await;
    let mut missing: Vec<&str> = Vec::new();

    for route in ROUTES {
        if call(&st, "TRACE", route.path(), None).await == StatusCode::NOT_FOUND {
            missing.push(route.path());
        }
    }

    missing.sort_unstable();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "these paths are in the table but the router does not serve them: {missing:#?}"
    );
}

#[tokio::test]
async fn the_table_is_not_empty_and_covers_every_guard_kind() {
    // Guards the guard. An empty or truncated table would make all three tests
    // above pass vacuously.
    assert!(
        ROUTES.len() > 150,
        "only {} routes in the table; the API has ~164",
        ROUTES.len()
    );
    for guard in [
        Guard::Admin,
        Guard::Staff,
        Guard::Auth,
        Guard::Device,
        Guard::InlineAuth,
        Guard::Public,
    ] {
        assert!(
            ROUTES.iter().any(|r| r.guard() == guard),
            "no route in the table has guard {guard:?}; either the API lost that \
             class entirely or the table was generated wrong"
        );
    }
}

#[tokio::test]
async fn the_database_is_never_reached_by_a_rejected_request() {
    // The claim the whole file rests on: rejection happens *before* the
    // database. If it did not, these 401s would actually be the database
    // refusing to answer, and the fixture would be measuring its own dead pool
    // rather than the authorization rules.
    //
    // Proven by contrast rather than by assertion about internals: a guarded
    // route with no credential is 401, while an unguarded route that does reach
    // the database is 500. Two different answers from the same fixture is what
    // shows the first one never got there.
    let st = state().await;

    let guarded = call(&st, "GET", "/api/auth/me", None).await;
    assert_eq!(guarded, StatusCode::UNAUTHORIZED);

    let reaches_db = call(&st, "GET", "/api/public/schedules", None).await;
    assert_eq!(
        reaches_db,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a public route that queries the database should surface the dead pool \
         as 500. If this is a 200, the fixture has a real database behind it \
         and the 401s in this file no longer prove what they claim."
    );
}

#[tokio::test]
async fn guarded_routes_are_not_asserted_for_role_gating() {
    // Not a test of the server — a test of this file's own honesty.
    //
    // It would be easy to read "164 routes × 7 credentials, all green" as "the
    // authorization matrix is covered". It is not: every case here carries *no*
    // valid user, so the difference between Admin, Staff, Member and Auth is
    // never exercised. A route whose guard was downgraded from AdminUser to
    // AuthUser would pass every assertion above.
    //
    // That distinction needs a real user, which needs a real database. This
    // records the boundary so the gap cannot be mistaken for coverage.
    let admin_routes = ROUTES.iter().filter(|r| r.guard() == Guard::Admin).count();
    let auth_routes = ROUTES.iter().filter(|r| r.guard() == Guard::Auth).count();
    assert!(
        admin_routes > 0 && auth_routes > 0,
        "the table distinguishes guard levels that this tier cannot verify; \
         see server/tests/live_db.rs for the half that can"
    );
}

/// How many route × credential pairs the offline matrix actually asserts.
fn asserted_pairs() -> usize {
    let shape_only = CREDS.iter().filter(|c| c.shape_only).count();
    ROUTES
        .iter()
        .filter(|r| r.is_guarded())
        .map(|r| {
            if matches!(r.guard(), Guard::Device | Guard::InlineAuth) {
                shape_only
            } else {
                CREDS.len()
            }
        })
        .sum()
}

#[tokio::test]
async fn the_offline_device_surface_is_exactly_this_narrow() {
    // The exemption above is the only place this file gives ground, so it is
    // pinned by number rather than left as a filter nobody counts.
    //
    // If someone widens it — by adding a guard kind to the `device_backed`
    // match, or by marking more credentials shape-only — the totals move and
    // this fails. That is the point: an exemption that can grow silently is
    // indistinguishable from no assertion at all.
    let device_routes = ROUTES
        .iter()
        .filter(|r| matches!(r.guard(), Guard::Device | Guard::InlineAuth))
        .count();
    let jwt_routes = ROUTES.iter().filter(|r| r.is_guarded()).count() - device_routes;

    assert_eq!(device_routes, 6, "device-authenticated routes");
    assert_eq!(jwt_routes, 139, "JWT-authenticated routes");
    assert_eq!(CREDS.iter().filter(|c| c.shape_only).count(), 3);
    assert_eq!(asserted_pairs(), 139 * 7 + 6 * 3);

    // And the rows that are *not* asserted here have somewhere to be. They are
    // the live-database tier's: a device token can only be rejected on its
    // contents by looking it up.
    let deferred = device_routes * (CREDS.len() - 3);
    assert_eq!(
        deferred, 24,
        "{deferred} route/credential pairs are deferred to the live-database \
         tier and are not covered by any assertion in this file"
    );
}

#[tokio::test]
async fn toolguard_parses_parameters_before_it_authenticates() {
    // A finding, recorded rather than fixed here.
    //
    // `tool_on`, `tool_off` and `tool_log` authenticate inside the handler body
    // (see `authorize_toolguard`), while every other guarded route in the API
    // uses an extractor. Extractors run first, so on those three the `Query`
    // extractor rejects a request with missing parameters *before* the
    // credential is ever examined — an unauthenticated caller gets 400 for a
    // bad `card` and 401 for a good one, which is a distinction it should not
    // be able to draw.
    //
    // The fix is to move the check into a `FromRequestParts` extractor, which
    // is a change to how the accepted credentials compose (device token OR API
    // key) and belongs with that work rather than smuggled in here.
    // `checks/tests/toolguard_auth.rs` already records the same divergence from
    // the other direction.
    let st = state().await;

    let no_params = call(&st, "GET", "/api/toolguard/does-not-take-query", None).await;
    assert_eq!(
        no_params,
        StatusCode::NOT_FOUND,
        "sanity: that path is not a route"
    );

    // With parameters, the credential is what is judged.
    let with_params = call(&st, "GET", "/api/toolguard/tool-on", None).await;
    assert_eq!(with_params, StatusCode::UNAUTHORIZED);

    // Without them, the parameter extractor answers first.
    let bare = axum::Router::new()
        .nest("/api", api::api_routes())
        .with_state(st.clone())
        .oneshot(
            Request::builder()
                .uri("/api/toolguard/tool-on")
                .body(Body::empty())
                .expect("well-formed request"),
        )
        .await
        .expect("infallible")
        .status();
    assert_eq!(
        bare,
        StatusCode::BAD_REQUEST,
        "if this is now 401, the check moved into an extractor and this test \
         should be deleted along with REQUIRED_QUERY"
    );
}
