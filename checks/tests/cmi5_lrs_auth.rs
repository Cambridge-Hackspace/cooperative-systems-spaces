//! Every cmi5 LRS handler must authenticate with `Cmi5SessionAuth`, and nothing
//! else.
//!
//! The LRS is the surface a launched cmi5 package talks to with its session
//! credential — semi-trusted third-party code running in a learner's browser.
//! The whole security boundary rests on those routes authenticating *only* via
//! the session credential, which is scoped to one registration/actor/activity.
//! A LRS handler that took `State` and `Query` and no auth extractor — the
//! ToolGuard mistake this check's sibling `toolguard_auth.rs` exists for — would
//! let anyone POST statements into any session and forge a pass.
//!
//! A LRS handler guarded by a *role* extractor (Admin/Staff/Member/Auth) would
//! be wrong in the other direction: those authenticate a logged-in user, not the
//! launched session, and would let a member post statements against a session
//! that is not theirs.
//!
//! Text-level on purpose: it needs no database, no `AppState` and no compiler,
//! so it runs on the FreeBSD workstation where `css-server` cannot be built. The
//! live route × credential matrix that supersedes it lands with the contract
//! tier (`Guard::Cmi5Session`).

use css_checks::read;

/// The LRS handlers, written out independently of the router so this check
/// states what ought to be true rather than agreeing with whatever the router
/// says.
const LRS_HANDLERS: &[&str] = &[
    "lrs_put_statement",
    "lrs_post_statement",
    "lrs_get_statements",
    "lrs_get_state",
    "lrs_put_state",
    "lrs_delete_state",
];

/// Role extractors that must NOT appear on a LRS handler.
const ROLE_EXTRACTORS: &[&str] = &[
    "AdminUser",
    "StaffUser",
    "MemberUser",
    "AuthUser",
    "DeviceAuth",
];

/// The signature of `async fn <name>(` up to its opening brace.
fn signature<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let at = src.find(&format!("async fn {name}("))?;
    let rest = &src[at..];
    let end = rest.find('{')?;
    Some(&rest[..end])
}

#[test]
fn the_scan_found_every_lrs_handler() {
    let src = read("server/src/api/cmi5.rs");
    for handler in LRS_HANDLERS {
        assert!(
            signature(&src, handler).is_some(),
            "LRS handler `{handler}` not found in api/cmi5.rs; this check would \
             pass over a route it is meant to guard. If a handler was renamed, \
             update LRS_HANDLERS."
        );
    }
}

#[test]
fn every_lrs_handler_authenticates_with_the_session_credential() {
    let src = read("server/src/api/cmi5.rs");
    let mut offenders = Vec::new();
    for handler in LRS_HANDLERS {
        let Some(sig) = signature(&src, handler) else {
            continue; // reported by the scan test above
        };
        if !sig.contains("Cmi5SessionAuth") {
            offenders.push(format!("{handler}: no Cmi5SessionAuth extractor"));
        }
    }
    assert!(
        offenders.is_empty(),
        "these LRS handlers do not authenticate via the session credential:\n{}\n\n\
         A LRS route without Cmi5SessionAuth is reachable by anyone who can \
         reach the server, which is exactly how forged statements would get in.",
        offenders.join("\n")
    );
}

#[test]
fn no_lrs_handler_is_guarded_by_a_role_instead() {
    let src = read("server/src/api/cmi5.rs");
    let mut offenders = Vec::new();
    for handler in LRS_HANDLERS {
        let Some(sig) = signature(&src, handler) else {
            continue;
        };
        for role in ROLE_EXTRACTORS {
            if sig.contains(role) {
                offenders.push(format!("{handler}: guarded by {role}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these LRS handlers use a role extractor instead of the session \
         credential:\n{}\n\n\
         A role extractor authenticates a logged-in user, not the launched \
         session, and would let one member post statements into another's \
         session.",
        offenders.join("\n")
    );
}

#[test]
fn the_lrs_routes_are_registered() {
    // Non-vacuity for the whole file: if the router stopped mounting the LRS
    // sub-paths, the handlers above could rot without any request reaching them.
    let src = read("server/src/api/cmi5.rs");
    assert!(
        src.contains("\"/lrs/statements\""),
        "the /lrs/statements route is not registered"
    );
    assert!(
        src.contains("\"/lrs/activities/state\""),
        "the /lrs/activities/state route is not registered"
    );
}
