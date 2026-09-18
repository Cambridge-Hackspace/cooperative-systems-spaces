//! Tier 4: every route × every credential state that carries no valid user.
//!
//! The whole route table x 7 credential states. Runs in-process over the real
//! router with a non-connecting database, so it needs no PostgreSQL and no
//! containers. (The exact route count is derived and asserted below, not quoted
//! here -- it grows with the API.)
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

async fn state() -> AppState {
    test_support::app_state().await
}

async fn call(st: &AppState, method: &str, path: &str, auth: Option<&str>) -> StatusCode {
    let mut b = Request::builder().method(method).uri(path);
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
        // A device token is opaque, so DeviceAuth must query to reject it; the
        // cmi5 session credential is the same shape of secret. Only their shape
        // checks are reachable without a database.
        let device_backed = matches!(
            route.guard(),
            Guard::Device | Guard::InlineAuth | Guard::Cmi5Session
        );

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
        "only {} routes in the table; the API has many more than that",
        ROUTES.len()
    );
    for guard in [
        Guard::Admin,
        Guard::Staff,
        Guard::Auth,
        Guard::Device,
        Guard::InlineAuth,
        Guard::Cmi5Session,
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
    // It would be easy to read "every route x 7 credentials, all green" as "the
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
            if matches!(
                r.guard(),
                Guard::Device | Guard::InlineAuth | Guard::Cmi5Session
            ) {
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
    let cmi5_session_routes = ROUTES
        .iter()
        .filter(|r| matches!(r.guard(), Guard::Cmi5Session))
        .count();
    let jwt_routes =
        ROUTES.iter().filter(|r| r.is_guarded()).count() - device_routes - cmi5_session_routes;

    // 10: the six pre-existing controller endpoints, the #43 power-report ingest,
    // the two #48 edge power endpoints (power-state poll + power-trip), and the
    // #83 module-state poll -- all InlineAuth (a device token or a per-tool/global
    // API key).
    assert_eq!(device_routes, 10, "device-authenticated routes");
    // The six cmi5 LRS routes: statements (PUT/POST/GET) and the State API
    // (GET/PUT/DELETE), all authenticated by the session credential. Like the
    // device surface, only their shape checks are reachable offline; their
    // contents are deferred to the live tier.
    assert_eq!(cmi5_session_routes, 6, "cmi5 LRS routes");
    // 190 = 140 base + 4 Groups.io + 8 membership/Stripe + 3 tool billing +
    // 8 cmi5 + 4 first-class cards (#33) + 3 training waivers (#36) + 17 power
    // topology (#42) + 1 power telemetry (#43) + 2 power re-enable (#44: the
    // circuit and tool emergency re-enable, both Staff; the #44 firmware self-trip
    // rides the existing power-report InlineAuth route). This number is the sum of
    // branches that each moved it, so it was re-derived by counting ROUTES rather
    // than by adding the comments together: guarded rows minus the device and
    // cmi5-session surfaces. The cmi5 `fetch` route and the Stripe and Groups.io
    // webhooks are Public; the six LRS routes are counted above.
    // 197 = 190 + 7 per-member rate tiers (#34: tier CRUD + list-assignments +
    // assign/clear, all Admin).
    // 198 = 197 + the RBAC read view (#65 Phase 3a: GET /api/admin/rbac, Admin).
    // 205 = 198 + the RBAC write API (#65 Phase 3c, all Admin): role create /
    // update / delete, set-permissions, set-inheritance, and user role
    // assign / unassign.
    // 206 = 205 + GET a user's assigned roles (#74, Admin), so the roster can
    // show which additional roles a user holds.
    // 195 = 206 - 11 for retiring the redundant tool-trainer / free-form
    // training-record subsystem (2 tools/{id}/trainers + 9 /api/trainers/**).
    // The structured instructor/step/session model is unchanged.
    // 202 = 195 + 7 tool module wiring (#83, all Admin): module list / create /
    // delete + the state snapshot, and interlock list / create / delete. The
    // matching device-facing module-state poll is InlineAuth and counted above.
    assert_eq!(jwt_routes, 202, "JWT-authenticated routes");
    assert_eq!(CREDS.iter().filter(|c| c.shape_only).count(), 3);
    assert_eq!(asserted_pairs(), 202 * 7 + (10 + 6) * 3);

    // And the rows that are *not* asserted here have somewhere to be. They are
    // the live-database tier's: a device or session token can only be rejected
    // on its contents by looking it up.
    let deferred = (device_routes + cmi5_session_routes) * (CREDS.len() - 3);
    assert_eq!(
        deferred, 64,
        "{deferred} route/credential pairs are deferred to the live-database \
         tier and are not covered by any assertion in this file"
    );
}

// The "member or above" tier is no longer empty: `POST /api/cmi5/aus/{id}/launch`
// is Guard::Member. The census in `the_offline_device_surface_is_exactly_this_narrow`
// counts it among the JWT-guarded routes and the 998-pair matrix asserts it
// refuses every invalid credential; the live "a Member is accepted, a Newbie is
// refused 403" case lives in the stack battery's contract stage (acceptance can
// only be shown against a database). The placeholder assertion that used to
// stand here — and `checks/tests/member_gate_is_dead.rs` — were removed when the
// gate went live, exactly as they instructed.

#[tokio::test]
async fn toolguard_judges_the_credential_before_the_parameters() {
    // This was a recorded finding until #107, and the inversion is the fix.
    //
    // `tool_on`, `tool_off` and `tool_log` authenticate inside the handler body
    // rather than through an extractor. Extractors run first, so while these
    // were `GET` with a required `Query<..>`, a request missing `card` was
    // rejected with 400 before the credential was ever examined -- and an
    // unauthenticated caller could therefore tell a well-formed request from a
    // malformed one, probing for valid parameters without holding anything.
    //
    // They are now `POST` with every wire field optional, validated *after*
    // `authorize_toolguard`. So a caller with no credential learns exactly one
    // thing -- that it has no credential -- whatever it sends.
    let st = state().await;

    assert_eq!(
        StatusCode::NOT_FOUND,
        call(&st, "POST", "/api/toolguard/does-not-exist", None).await,
        "sanity: that path is not a route, so a 401 below means the route matched"
    );

    // An empty body, which used to be the cheap way to get a 400 out of it.
    let bare = axum::Router::new()
        .nest("/api", api::api_routes())
        .with_state(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/toolguard/tool-on")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("well-formed request"),
        )
        .await
        .expect("infallible")
        .status();
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        bare,
        "an unauthenticated caller must not be able to distinguish a complete \
         request from an incomplete one -- that distinction is what let it probe \
         for valid parameters while holding no credential at all"
    );

    // No content-type at all, which is how this was caught. Optional fields
    // were not enough on their own: `Json<..>` rejects a missing content-type
    // with 415 before the handler runs, so the first version of this fix moved
    // the probing oracle from the query extractor to the body extractor
    // instead of removing it. The handlers take `Bytes` for exactly this.
    let no_content_type = axum::Router::new()
        .nest("/api", api::api_routes())
        .with_state(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/toolguard/tool-on")
                .body(Body::empty())
                .expect("well-formed request"),
        )
        .await
        .expect("infallible")
        .status();
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        no_content_type,
        "a request with no content-type and no credential must be refused for \
         the credential. 415 here means a body extractor is running before \
         authentication again"
    );

    // And a complete one answers the same, so the 401 above is about the
    // credential rather than about the body being empty.
    let complete = axum::Router::new()
        .nest("/api", api::api_routes())
        .with_state(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/toolguard/tool-on")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"card":"x","tool_id":"y"}"#))
                .expect("well-formed request"),
        )
        .await
        .expect("infallible")
        .status();
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        complete,
        "a complete body with no credential must answer identically to an empty one"
    );
}
