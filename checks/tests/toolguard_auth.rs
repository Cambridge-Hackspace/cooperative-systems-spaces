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
//! requests carry an `api_key` field that nothing read, and `validate_api_key`
//! — which checks a tool's `external_api_key` and the global
//! `toolguard.global_api_key` — was called from nowhere in the crate.
//!
//! This is a text-level check on purpose. It needs no database, no `AppState`
//! and no compiler, so it runs on the FreeBSD workstation where `css-server`
//! cannot be built at all — which is where the defect would ideally have been
//! caught. The full route × credential matrix that supersedes it needs a live
//! router and lands with the server contract tier.

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
         with a registered device's Bearer token (extract_device_auth) or a valid API key."
    );
}

#[test]
fn the_api_key_mechanism_is_actually_wired_in() {
    // `validate_api_key` existed, was correct, and was called from nowhere;
    // the `api_key` request fields were parsed and never read. Dead security
    // machinery is worse than none, because its presence implies a check that
    // is not happening.
    let src = read("server/src/api/toolguard.rs");
    let definition = src.matches("async fn validate_api_key").count();
    let total = src.matches("validate_api_key").count();

    assert_eq!(
        definition, 1,
        "validate_api_key should be defined exactly once"
    );
    assert!(
        total > definition,
        "validate_api_key is defined but never called. Either wire it in, or delete it \
         together with the api_key request fields — leaving it implies a key check that \
         does not happen."
    );
}
