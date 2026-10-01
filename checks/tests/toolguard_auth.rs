//! Every ToolGuard endpoint must authenticate its caller.
//!
//! This exists because three of them did not. `tool-on`, `tool-off` and
//! `tool-log` took `State` and `Query` and nothing else — so
//! `GET /api/toolguard/tool-on?card=…&tool_id=…` energised a machine for
//! anyone who could reach the server, over a plain URL, with no credential of
//! any kind. `sync` and `boot-reset`, in the same file, called
//! `extract_device_auth` correctly.
//!
//! The mechanism intended to stop it was fully written and never wired in: the
//! requests carried an `api_key` field that nothing read, and `validate_api_key`
//! — which checked a tool's `external_api_key` and the global
//! `toolguard.global_api_key` — was called from nowhere in the crate. #101 slice 6
//! deleted all three rather than wiring them in, so one credential remains: a
//! registered device's Bearer token, scoped by an explicit binding. What this file
//! now guards is that they stay gone.
//!
//! This is a text-level check on purpose. It needs no database, no `AppState`
//! and no compiler, so it runs on the FreeBSD workstation where `css-server`
//! cannot be built at all — which is where the defect would ideally have been
//! caught. The full route × credential matrix that supersedes it needs a live
//! router and lands with the server contract tier.
//!
//! Those three are `POST` now, and their card travels in a JSON body rather
//! than a URL (#107) — so the shape quoted above no longer exists. It is kept
//! as the reason this file was written, not as a description of the current
//! surface. The check itself has never cared about the method: it matches
//! `(tool_on)` inside `toolguard_routes()`, so it reads `post(..)` exactly as
//! it read `get(..)`.

use css_checks::read;

/// Handlers registered by `toolguard_routes()`, paired with how they are
/// allowed to authenticate.
///
/// Written out rather than derived. The point of this check is to state
/// independently what the routing table ought to look like; deriving it from
/// the routing table would agree with the routing table no matter what it said.
const EXPECTED: &[(&str, Auth)] = &[
    // A static liveness probe. It reads no state, takes no parameters, and
    // returns a constant, so there is nothing for a credential to protect.
    ("api_status", Auth::PublicByDesign),
    ("tool_on", Auth::Required),
    ("tool_off", Auth::Required),
    ("tool_log", Auth::Required),
    ("sync", Auth::Required),
    ("boot_reset", Auth::Required),
    // #43 power telemetry ingest: reports a tool's latest reading, authenticated
    // like the other controller endpoints (a device token bound to the tool).
    ("power_report", Auth::Required),
    // #48 edge power lockout: the edge polls the lockout+topology snapshot
    // (device-authed, like sync) and reports an edge_fast_trip.
    ("power_state", Auth::Required),
    ("power_trip", Auth::Required),
    // #83 tool module bindings + interlocks: the edge polls the wiring snapshot
    // it coordinates from, device-authed exactly like power_state.
    ("module_state", Auth::Required),
];

#[derive(PartialEq, Eq, Debug)]
enum Auth {
    Required,
    PublicByDesign,
}

/// The functions that constitute authenticating a ToolGuard caller.
const AUTHORIZERS: &[&str] = &["extract_device_auth", "authorize_toolguard"];

/// The body of `async fn <name>(`, ending at the next top-level item.
///
/// "Next item" has to mean any column-0 `fn`/`pub fn`/`async fn`/`pub async fn`,
/// not just `async fn`. An earlier version of this stopped only at
/// `\nasync fn `, so `tool_log` — which is immediately followed by
/// `pub async fn extract_device_auth` — absorbed that function into its body,
/// found the word `extract_device_auth` inside it, and was reported as
/// authenticating when it did nothing of the kind. The check found two of the
/// three real defects and silently exonerated the third.
fn handler_body<'a>(src: &'a str, name: &str) -> &'a str {
    let sig = format!("async fn {name}(");
    let start = src.find(&sig).unwrap_or_else(|| {
        panic!("no handler `{name}` in api/toolguard.rs — has it been renamed?")
    });
    let rest = &src[start + sig.len()..];

    let end = rest
        .match_indices('\n')
        .find(|(i, _)| {
            let line = rest[i + 1..].split('\n').next().unwrap_or("");
            ["fn ", "pub fn ", "async fn ", "pub async fn "]
                .iter()
                .any(|kw| line.starts_with(kw))
        })
        .map(|(i, _)| i);

    match end {
        Some(end) => &rest[..end],
        None => rest,
    }
}

#[test]
fn the_routing_table_registers_exactly_the_handlers_this_check_knows_about() {
    // Guards the guard. If a route is added and this list is not updated, the
    // per-handler assertion below would never look at it and would keep
    // reporting a clean bill of health over a shrinking set.
    let src = read("server/src/api/toolguard.rs");
    let routes_start = src
        .find("pub fn toolguard_routes()")
        .expect("toolguard_routes() must exist");
    let routes = &src[routes_start..];
    let routes = &routes[..routes.find("\n}").expect("unterminated toolguard_routes()")];

    for (handler, _) in EXPECTED {
        assert!(
            routes.contains(&format!("({handler})")),
            "handler `{handler}` is no longer registered by toolguard_routes(); \
             either it moved or this check is now watching a route that does not exist"
        );
    }

    // And nothing registered that we do not know about.
    let registered = routes.matches(".route(").count();
    assert_eq!(
        registered,
        EXPECTED.len(),
        "toolguard_routes() registers {registered} routes but this check knows about {}. \
         A new ToolGuard endpoint must be added to EXPECTED with its authentication \
         requirement stated.",
        EXPECTED.len()
    );
}

#[test]
fn every_toolguard_handler_authenticates_its_caller() {
    let src = read("server/src/api/toolguard.rs");

    let unauthenticated: Vec<&str> = EXPECTED
        .iter()
        .filter(|(_, auth)| *auth == Auth::Required)
        .map(|(name, _)| *name)
        .filter(|name| {
            let body = handler_body(&src, name);
            !AUTHORIZERS.iter().any(|a| body.contains(a))
        })
        .collect();

    assert!(
        unauthenticated.is_empty(),
        "these ToolGuard handlers accept a request without authenticating it: {unauthenticated:?}. \
         They control physical machinery and are reachable by URL, so an unauthenticated \
         one lets anybody who can reach the server energise or kill a tool. Authenticate \
         with a registered device's Bearer token (extract_device_auth), bound to the tool."
    );
}

/// The retired credentials stay retired.
///
/// This replaces `the_api_key_mechanism_is_actually_wired_in`, which asserted the
/// opposite: `validate_api_key` existed, was correct, and was called from nowhere,
/// so that test demanded it be wired in *or* deleted. #101 slice 6 chose deletion
/// -- the weakest accepted credential sets the real bar, and one of the two was a
/// single shared secret that opened every tool.
///
/// The principle the old test encoded is the one kept here: dead or weak security
/// machinery is worse than none, because its presence implies a check that is not
/// happening. So this asserts ABSENCE, which is the direction that matters now --
/// a reintroduced key path would be a silent widening of what may actuate a
/// machine, and it would look like a convenience.
#[test]
fn the_retired_credentials_are_not_back() {
    let src = read("server/src/api/toolguard.rs");
    let cfg = read("server/src/config.rs");

    // Comments are stripped so the prose explaining the retirement -- which names
    // all three by design -- cannot satisfy the assertions it is describing.
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !code.contains("fn validate_api_key"),
        "`validate_api_key` is back in toolguard.rs. Tool authentication is a \
         registered device's Bearer token scoped by a `device_bindings` row; an \
         API-key path alongside it means the weaker credential decides what can \
         energise a machine."
    );
    assert!(
        !code.contains("api_key"),
        "an `api_key` field or lookup is back on the ToolGuard surface. The \
         request types carried one for a mechanism that was never wired in, and \
         #101 slice 6 removed both; re-adding one reintroduces a credential that \
         is not hashed at rest and is not scoped by a binding."
    );
    assert!(
        !cfg.contains("global_api_key"),
        "`toolguard.global_api_key` is back in the configuration. It was ONE \
         shared secret that authenticated every tool, so a single leak opened the \
         whole space -- which is why it went rather than being documented."
    );
}

/// A billable report needs the `power` binding, not merely any binding.
///
/// Entry auth accepts any binding, so a `reader` may start a tool. Posting money
/// against it is narrower on purpose: that is the property `metered_key_ok`
/// provided by demanding the tool's own key, and it has to survive the move to
/// device tokens rather than being quietly relaxed to "any authenticated device".
#[test]
fn the_metered_gate_requires_the_power_binding() {
    let src = read("server/src/api/toolguard.rs");
    let body = handler_body(&src, "metered_device_ok");

    assert!(
        body.contains("device_is_bound_to_tool_in_role"),
        "the metered gate no longer asks for a ROLE-scoped binding. If it accepts \
         any binding, a reader wired to a metered tool can post charges for it."
    );
    assert!(
        body.contains("binding_role::POWER"),
        "the metered gate no longer requires the `power` role specifically. A \
         billable report must come from the thing that actually switches the tool."
    );
}
