//! A device's `/sync` payload carries only what it is bound to (#101 slice 5).
//!
//! `GET /api/toolguard/sync` hands a device the tools it may actuate and the
//! members authorized for them -- card digests included. It used to take a device
//! id at the API layer, echo it back, and answer with EVERY tool and EVERY
//! member's identifier, so one compromised reader yielded the whole membership
//! plus the full authorization matrix. #104 scoped it to the device's bindings.
//!
//! **Why a second oracle.** The e2e `toolmodules` stage asserts the payload from
//! both sides -- a reader bound to one tool receives that tool and not another
//! device's, in both directions, and receives no identifier for a member
//! authorized for nothing it serves. That is the behavioural half, and it is the
//! stronger one. What it cannot see is *why* the payload was narrow: a scope that
//! happens to be empty in the fixture, or one that narrows by accident downstream,
//! looks identical from outside. This file pins the mechanism, so a change that
//! widens the query fails here even if the fixture would not have noticed.
//!
//! The three things asserted are the three that must all hold for the narrowing to
//! be real:
//!
//!   1. the scope comes from the REQUESTING device's bindings, not from a place, a
//!      capability or anything else the topology merely implies;
//!   2. it is restricted to TOOL resources -- since #101 a binding can name any
//!      resource, and a device that coordinates a door has that door's id among
//!      its bindings;
//!   3. the tool query is actually gated by that scope rather than loading every
//!      tool and narrowing in Rust afterwards.
//!
//! What this does NOT prove: that the payload is correct, or that the members
//! attached to a bound tool are the right ones. That is the e2e stage's job. It
//! proves the query cannot silently stop being scoped.
//!
//! Text-level, like its neighbours: no database and no compiler, so it runs on the
//! workstation where `css-server` cannot be built.

use css_checks::read;

const DATABASE: &str = "server/src/database.rs";

/// The body of a named method, from its signature to the next line that is a lone
/// `}` at four-space indentation, with line comments stripped.
///
/// Stripping matters in both directions here: the prose in this function explains
/// what the old unscoped version did, and an unstripped scan would read the
/// explanation as the code. Same reasoning as `tool_access_agrees.rs`.
fn method_body(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("pub fn {name}("))
        .unwrap_or_else(|| panic!("no method named `{name}` in {DATABASE}"));
    let rest = &source[start..];
    let end = rest.find("\n    }\n").unwrap_or(rest.len());
    rest[..end]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sync_body() -> String {
    method_body(&read(DATABASE), "get_toolguard_sync_data")
}

#[test]
fn the_sync_method_was_actually_found() {
    // Anti-vacuity. A rename would make every assertion below run over an empty
    // string and pass in silence -- which for a check guarding a payload of card
    // digests is the worst way to fail.
    let body = sync_body();
    assert!(
        body.len() > 400,
        "`get_toolguard_sync_data` parsed as {} bytes, too short to be the method. \
         If it was renamed, re-point this file and re-derive the claims rather than \
         assuming the scoping survived the rename.",
        body.len()
    );
}

#[test]
fn the_scope_comes_from_the_requesting_devices_bindings() {
    let body = sync_body();
    assert!(
        body.contains("device_bindings::device_id.eq(device_id)"),
        "the sync payload's scope is no longer derived from the requesting \
         device's bindings.\n\n\
         This is the narrowing that stops one compromised reader from yielding \
         every tool and every member's card digest. Scope must be an explicit \
         grant an administrator made -- a binding row -- not something inferred \
         from a place, a device capability, or the power topology."
    );
}

#[test]
fn the_scope_is_restricted_to_tool_resources() {
    let body = sync_body();
    assert!(
        body.contains("inner_join(tools::table"),
        "the sync scope query no longer joins `tools`.\n\n\
         Since #101 a binding may name ANY resource: a device that coordinates a \
         door has that door's id among its bindings. Without the join, a door id \
         enters the tool authorization scope. Nothing leaks today only because a \
         door id matches no tool -- and the scope of a security-relevant query \
         must not depend on a downstream lookup happening to fail."
    );
}

#[test]
fn the_tool_query_is_gated_by_that_scope() {
    let body = sync_body();
    assert!(
        body.contains("tools::id.eq_any(&bound)"),
        "the tool list is no longer filtered by the bound set.\n\n\
         Deriving `bound` and then loading every tool anyway would leave the \
         narrowing to whatever happens downstream. The query itself has to be \
         scoped, so that a later change to the per-user authorization logic \
         cannot widen what the device is told about."
    );
}

// ── Self-tests ───────────────────────────────────────────────────────────────
//
// Each assertion above is fed the shape it exists to reject, because a check on a
// security property that has never been observed failing is a check of unmeasured
// value.

#[cfg(test)]
mod the_checks_reject_what_they_are_for {
    use super::*;

    /// The pre-#104 shape: takes a device id, ignores it, answers with everything.
    const UNSCOPED: &str = r#"pub fn get_toolguard_sync_data(
        &self,
        device_id: uuid::Uuid,
    ) -> Result<(), DatabaseError> {
        let all_users = users::table.load(&mut conn)?;
        let all_tools = tools::table.load(&mut conn)?;
        Ok(())
    }
"#;

    #[test]
    fn an_unscoped_sync_is_caught() {
        let body = method_body(UNSCOPED, "get_toolguard_sync_data");
        assert!(
            !body.contains("device_bindings::device_id.eq(device_id)"),
            "the unscoped fixture must not appear to derive a scope"
        );
        assert!(
            !body.contains("tools::id.eq_any(&bound)"),
            "the unscoped fixture must not appear to gate its tool query"
        );
    }

    /// The subtler regression: scoped by device, but not restricted to tools, so a
    /// door binding widens the tool scope.
    #[test]
    fn a_scope_that_is_not_restricted_to_tools_is_caught() {
        let body = "pub fn get_toolguard_sync_data(\n        \
                    device_bindings::table.filter(device_bindings::device_id.eq(device_id))\n    }\n";
        let parsed = method_body(body, "get_toolguard_sync_data");
        assert!(parsed.contains("device_bindings::device_id.eq(device_id)"));
        assert!(
            !parsed.contains("inner_join(tools::table"),
            "a device-scoped but resource-unrestricted query must still be reported"
        );
    }

    /// And the comment-stripping, which this file depends on: the real method's
    /// prose describes the unscoped behaviour it replaced, so an unstripped scan
    /// could satisfy an assertion from the explanation rather than the code.
    #[test]
    fn prose_does_not_satisfy_an_assertion() {
        let commented = "pub fn get_toolguard_sync_data(\n        \
                         // was: tools::id.eq_any(&bound)\n        let x = 1;\n    }\n";
        let parsed = method_body(commented, "get_toolguard_sync_data");
        assert!(
            !parsed.contains("tools::id.eq_any(&bound)"),
            "a mention in a comment must not count as the query being scoped"
        );
    }
}
