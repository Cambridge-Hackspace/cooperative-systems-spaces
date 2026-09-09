//! Both answers to "may this user use this tool" come from one rule.
//!
//! There are two callers and they are not interchangeable. The web API asks
//! `can_access_tool` -- `check_tool_access`, `check_my_tool_access`, and the
//! `can_access_tool` field of every `ToolTrainingOverview` the frontend
//! renders. The toolguard sync path, which is what a physical machine
//! interlock acts on, builds its allow-list in `get_toolguard_sync_data`. As of
//! #36 both resolve access through one shared rule, `user_is_authorized_for_tool`.
//!
//! They used to be separate implementations, and they disagreed.
//! `can_access_tool` tested only that a `user_training_progress` row existed --
//! `count() > 0`, reading neither `status` nor `expires_at`. Since
//! `start_training_session` is self-serve for your own user, ungated, and
//! upserts, any member could grant themselves web-reported access to any tool
//! by pressing Start Training once per step. An expired certification read as
//! access. So did a step whose status was `failed`.
//!
//! The physical guard refused all three, so the divergence presented as a web
//! UI that told members they were cleared for machines the interlock would not
//! release -- the safe direction by luck rather than design, and the reverse
//! would have been a member cleared by nothing at all.
//!
//! A text-level check on purpose: it needs no database and no compiler, so it
//! runs on the FreeBSD workstation where `css-server` cannot be built.
//!
//! What this does NOT prove: that the shared rule is *correct* at runtime, or
//! that either caller is reached. It proves there is one rule rather than two,
//! and that the rule still reads the two columns whose absence was the bug.

use css_checks::read;

fn database_source() -> String {
    read("server/src/database.rs")
}

/// The body of a named method, from its signature to the next line that is a
/// lone `}` at four-space indentation -- how an `impl` block closes a method,
/// with line comments stripped.
///
/// Stripping matters in both directions, and this file proved it: the prose
/// explaining why `can_access_tool` must not query user_training_progress
/// names the table, so an unstripped scan reads the explanation as the
/// offence. The mirror case is worse -- a presence assertion satisfied by a
/// comment that merely mentions the call it is looking for. Same reasoning as
/// route_parity.rs:79 and cli_api_paths.rs:37.
fn method_body(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("pub fn {name}("))
        .unwrap_or_else(|| panic!("no method named `{name}` in server/src/database.rs"));
    let rest = &source[start..];
    let end = rest.find("\n    }\n").unwrap_or(rest.len());
    rest[..end]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn both_methods_were_actually_found() {
    // Anti-vacuity. A rename would make every assertion below run over an
    // empty string and pass in silence.
    let source = database_source();
    for name in ["can_access_tool", "user_has_completed_all_training_steps"] {
        let body = method_body(&source, name);
        assert!(
            body.len() > 120,
            "`{name}` parsed as {} bytes, which is too short to be the method. \
             If it was renamed, re-derive the rules recorded here rather than \
             re-pointing this file at whatever now has the old name.",
            body.len()
        );
    }
}

#[test]
fn the_web_path_delegates_rather_than_re_deriving() {
    let body = method_body(&database_source(), "can_access_tool");

    assert!(
        body.contains("self.user_is_authorized_for_tool(user_id, tool_id, tool.requires_training)"),
        "`can_access_tool` no longer delegates to the shared rule \
         `user_is_authorized_for_tool` (which owns step-completion, the active-\
         waiver check, and the requires_training flag).\n\n\
         Two implementations of one rule is what this check exists to stop. If \
         the web path needs an answer the other callers do not give, change the \
         shared function and say why every caller wants the new behaviour -- do \
         not grow a second copy here."
    );
}

#[test]
fn the_web_path_has_no_bare_existence_test() {
    let body = method_body(&database_source(), "can_access_tool");

    assert!(
        !body.contains(".count()"),
        "`can_access_tool` counts rows again.\n\n\
         The defect this replaced was `count() > 0` on user_training_progress: \
         a row existing is not training completed. `start_training_session` \
         upserts a row for any user who presses Start on their own training, so \
         an existence test hands out access to anyone who asks for it."
    );
    assert!(
        !body.contains("user_training_progress"),
        "`can_access_tool` queries user_training_progress directly again. \
         Reading the progress table here is how the two rules drifted apart the \
         first time; the shared function owns that query."
    );
}

#[test]
fn the_shared_rule_still_reads_status_and_expiry() {
    let body = method_body(&database_source(), "user_has_completed_all_training_steps");

    assert!(
        body.contains("TrainingStatus::Completed"),
        "the shared rule no longer compares status to Completed, so an \
         in-progress or failed step counts as training."
    );
    assert!(
        body.contains("user_training_progress::expires_at"),
        "the shared rule no longer selects expires_at, so `expires_after_days` \
         is decorative and an expired certification never lapses."
    );
    assert!(
        body.contains("chrono::Utc::now()"),
        "the shared rule selects expires_at but no longer compares it to now, \
         which computes an expiry nothing acts on."
    );
}

// The divergence this file used to pin -- the web path honouring
// `requires_training` while the toolguard sync path ignored it -- was closed on
// purpose by #36. Both paths now resolve access through the one shared rule
// `user_is_authorized_for_tool`, which reads step completion, an active waiver,
// AND `requires_training`. The old test said, in its own failure message, to
// delete it and state what the machine interlock now releases: a
// `requires_training` tool with no steps is now GATED (opened by a waiver -- the
// migrated-ToolPass case) rather than open, in the sync path as it already was
// on the web. Replaced by a convergence assertion so "one rule" stays pinned.
#[test]
fn both_access_paths_share_one_rule() {
    let source = database_source();
    let web = method_body(&source, "can_access_tool");
    let sync = method_body(&source, "get_toolguard_sync_data");

    for (name, body) in [
        ("can_access_tool", &web),
        ("get_toolguard_sync_data", &sync),
    ] {
        assert!(
            body.contains("self.user_is_authorized_for_tool("),
            "`{name}` no longer resolves tool access through the shared rule \
             `user_is_authorized_for_tool`. Both paths must call it, or the web \
             self-report and the machine interlock can disagree again."
        );
    }

    // The shared rule must still consider an active waiver, or a granted waiver
    // silently confers no access.
    let rule = method_body(&source, "user_is_authorized_for_tool");
    assert!(
        rule.contains("user_has_active_waiver"),
        "the shared authorization rule no longer checks for an active waiver, \
         so waivers (and the migrated ToolPass grants stored as waivers) grant \
         no access."
    );
}

/// Phase 2 added a second access dimension -- metered-billing affordability --
/// on top of training. It must hold on BOTH the web self-check and the physical
/// guard's allow-list, exactly like training, or the two silently disagree: the
/// web UI promises access the machine denies (or vice versa). This pins the new
/// dimension on both halves the same way the training rule is pinned above.
#[test]
fn the_metered_billing_gate_holds_on_both_paths() {
    let source = database_source();
    let web = method_body(&source, "can_access_tool");
    let sync = method_body(&source, "get_toolguard_sync_data");

    assert!(
        web.contains("metered_gate") && web.contains("gate.authorizes"),
        "`can_access_tool` no longer applies the metered-billing gate, so the \
         web self-report would promise access to a metered tool the member \
         cannot afford -- the machine would then refuse it. Reapply the gate on \
         this path, or unify the two and delete this test with a reason."
    );
    assert!(
        sync.contains("metered_gate") && sync.contains("gate.authorizes"),
        "`get_toolguard_sync_data` no longer applies the metered-billing gate, \
         so the edge allow-list would offer a metered tool the member cannot \
         afford -- and in edge-local mode the tool would energize before the \
         server could refuse it. Reapply the gate on this path."
    );
}
