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
///
/// `authorize_power_report` joined these with #161: a module behind an edge has
/// no site credentials, so its reading arrives signed by the edge, and that
/// handler needs a rule the plain one cannot express (the caller bound as the
/// tool's `edge`, the named module bound as its `power`). It authenticates by
/// delegating rather than by calling the primitive itself.
const AUTHORIZERS: &[&str] = &[
    "extract_device_auth",
    "authorize_toolguard",
    "authorize_power_report",
];

/// The one that actually reads the credential. Everything else in
/// [`AUTHORIZERS`] has to reach this, which
/// [`every_authorizer_reaches_the_primitive`] is what proves.
const PRIMITIVE_AUTHORIZER: &str = "extract_device_auth";

/// Strip `//` line comments, so prose naming a function cannot stand in for a
/// call to it.
///
/// Found by mutation: `authorize_power_report` was hollowed out until it
/// authenticated nothing, and [`every_authorizer_reaches_the_primitive`] passed
/// regardless — its docstring and an inline comment both name
/// `authorize_toolguard`, and a substring search cannot tell prose from code.
/// Two source checks in this repository already carry scars from exactly this.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

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

/// Walk the call graph from `name`, following only names on `authorizers`, and
/// say whether `primitive` is reached.
///
/// A WALK, NOT ONE HOP. The previous version asked whether a body mentioned
/// *some other* name on the list and called that reaching the primitive — so two
/// authorizers naming each other satisfied it with neither reading a credential,
/// and the test's own name was the only thing asserting otherwise. [`AUTHORIZERS`]
/// says a name on it is trusted; this is what stops that trust being circular.
///
/// `primitive` is recognised when it appears in a body, so its own body is never
/// taken — the walk does not depend on where it is defined.
fn reaches(src: &str, name: &str, authorizers: &[&str], primitive: &str) -> bool {
    let mut seen: Vec<&str> = vec![name];
    let mut frontier: Vec<&str> = vec![name];

    while let Some(cur) = frontier.pop() {
        let body = code_only(handler_body(src, cur));
        for next in authorizers.iter().filter(|n| **n != cur) {
            if !body.contains(next) {
                continue;
            }
            if *next == primitive {
                return true;
            }
            if !seen.contains(next) {
                seen.push(next);
                frontier.push(next);
            }
        }
    }

    false
}

/// A name on the [`AUTHORIZERS`] list has to earn it.
///
/// The handler check above is satisfied by a handler *mentioning* any of these,
/// so the list is trusted: add a name that does not authenticate and every
/// handler calling it is silently exonerated — the same shape as the absorption
/// bug `handler_body` documents, one level up. So each delegating authorizer must
/// be shown to reach the primitive that reads the credential, rather than taken
/// on the list's word.
#[test]
fn every_authorizer_reaches_the_primitive() {
    let src = read("server/src/api/toolguard.rs");

    let hollow: Vec<&str> = AUTHORIZERS
        .iter()
        .filter(|a| **a != PRIMITIVE_AUTHORIZER)
        .filter(|a| !reaches(&src, a, AUTHORIZERS, PRIMITIVE_AUTHORIZER))
        .copied()
        .collect();

    assert!(
        hollow.is_empty(),
        "these names are trusted as authorizers but never reach `{PRIMITIVE_AUTHORIZER}`: \
         {hollow:?}. Any handler calling one of them is reported as authenticated while \
         accepting an unauthenticated request."
    );
}

/// The walk must reject a cycle, because that is what the one-hop version
/// accepted.
///
/// Fed the shape of the real defect rather than a shape invented to pass: two
/// authorizers that name each other and nothing else. The one-hop predicate
/// answered "both delegate to an authorizer" and the suite went green with no
/// credential read anywhere on that path. The positive half is here too — a
/// genuine chain must still be accepted, or a correct `every_authorizer_reaches_the_primitive`
/// would start failing on the truth.
#[test]
fn the_reachability_walk_rejects_a_cycle_and_accepts_a_real_chain() {
    const AUTH: &[&str] = &["prim", "alpha", "beta"];

    let cyclic = "\
async fn alpha(req: Request) -> Response {
    beta(req).await
}
async fn beta(req: Request) -> Response {
    alpha(req).await
}
";
    assert!(
        !reaches(cyclic, "alpha", AUTH, "prim"),
        "alpha and beta name only each other, so no credential is ever read — \
         the walk accepted a cycle, which is the defect this test exists for"
    );
    assert!(
        !reaches(cyclic, "beta", AUTH, "prim"),
        "the same cycle entered from the other end must also be rejected"
    );

    let chain = "\
async fn alpha(req: Request) -> Response {
    beta(req).await
}
async fn beta(req: Request) -> Response {
    prim(req).await
}
";
    assert!(
        reaches(chain, "alpha", AUTH, "prim"),
        "alpha -> beta -> prim is a real delegation and must be accepted, or the \
         check rejects correct code"
    );

    // A comment naming the primitive must not count, for `code_only`'s reason:
    // prose cannot stand in for a call.
    let commented = "\
async fn alpha(req: Request) -> Response {
    // delegates to prim
    beta(req).await
}
async fn beta(req: Request) -> Response {
    alpha(req).await
}
";
    assert!(
        !reaches(commented, "alpha", AUTH, "prim"),
        "a comment naming `prim` is prose, not a call, and must not satisfy the walk"
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
            let body = code_only(handler_body(&src, name));
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

/// Being bound to the tool is the whole authorization to put money on it, and a
/// report that names no card is attributed through the tool's session (#174).
///
/// Until #174 a narrower gate (`metered_device_ok`) demanded the `power`
/// binding for `tool-on` and `tool-log` on a metered tool, which made a tool
/// with a separate badge reader and plug unstartable by badge: the reader held
/// no `power` binding and the plug held no card. The owner's decision was that
/// the server already knows who is signed on to a tool, so a device that cannot
/// name a card names the tool alone. These are the two properties that decision
/// rests on, and either regressing silently would reopen #174 from one side or
/// the other.
#[test]
fn a_cardless_report_is_attributed_through_the_tools_session() {
    let src = read("server/src/api/toolguard.rs");
    let body = handler_body(&src, "signed_on_user");

    assert!(
        body.contains("open_tool_session_for_tool"),
        "a card-less report is no longer attributed to the open billing session's \
         member -- a plug's usage on a metered tool would land on nobody."
    );
    assert!(
        body.contains("latest_activation") && body.contains("ToolStatus::InUse"),
        "a card-less report on an UNMETERED tool is no longer attributed to the \
         member whose activation put it in use -- or is, without checking the tool \
         is still in use, which would bill the last member for the next one's time."
    );

    let log = handler_body(&src, "tool_log");
    assert!(
        log.contains("signed_on_user"),
        "tool_log no longer consults signed_on_user: a report naming no card has \
         nobody to go to and a plug cannot meter anything."
    );
}

/// The `power` binding is not what authorizes a charge any more (#174). Entry
/// auth -- a device token bound to THIS tool, in any role -- is.
///
/// The relay rules on the power report and the alive pulse still ask for roles,
/// as they should: a relayed liveness claim is about a specific device. The
/// three money-moving handlers must not.
#[test]
fn the_tool_handlers_do_not_gate_on_the_power_role() {
    let src = read("server/src/api/toolguard.rs");
    for handler in ["tool_on", "tool_off", "tool_log"] {
        let body = handler_body(&src, handler);
        assert!(
            !body.contains("binding_role::POWER"),
            "{handler} asks for the `power` role again. A reader beside a separate \
             plug then cannot start, report on or stop a metered tool, which is #174."
        );
    }
}
