//! Both answers to "may this user use this tool" come from one rule.
//!
//! There are two callers and they are not interchangeable. The web API asks
//! `can_access_tool` -- `check_tool_access`, `check_my_tool_access`, and the
//! `can_access_tool` field of every `ToolTrainingOverview` the frontend
//! renders. The toolguard sync path, which is what a physical machine
//! interlock acts on, asks `user_has_completed_all_training_steps`.
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
        body.contains("self.user_has_completed_all_training_steps(user_id, tool_id)"),
        "`can_access_tool` no longer delegates to \
         `user_has_completed_all_training_steps`.\n\n\
         Two implementations of one rule is what this check exists to stop. If \
         the web path needs an answer the sync path does not give, change the \
         shared function and say why both callers want the new behaviour -- do \
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

#[test]
fn the_remaining_divergence_is_still_the_one_recorded_here() {
    // Not a defect being asserted as correct -- a difference being held still
    // so that closing it is a decision somebody makes on purpose.
    //
    // `can_access_tool` short-circuits on `tool.requires_training`. The sync
    // path keys off `tool_has_training_steps` and never reads that flag. A tool
    // with `requires_training = false` and training steps configured is
    // therefore open on the web and gated at the machine.
    //
    // Left alone deliberately: the fix that unified the *completion* rule was
    // strictly narrowing on the web path and could only revoke access it should
    // never have granted. Teaching the physical guard to honour a flag that
    // turns training off would *widen* what an interlock releases, which is not
    // a change to make as a side effect of anything.
    let source = database_source();
    let web = method_body(&source, "can_access_tool");
    let sync = method_body(&source, "get_toolguard_sync_data");

    assert!(
        web.contains("tool.requires_training"),
        "`can_access_tool` no longer honours `requires_training`. If the two \
         paths were unified, delete this test and say in the commit message \
         what the physical guard now releases that it did not before."
    );
    assert!(
        sync.contains("self.tool_has_training_steps(tool.id)")
            && !sync.contains("requires_training"),
        "the toolguard sync path now reads `requires_training`, so the \
         divergence recorded here is closed. That is a change to what a machine \
         interlock releases: delete this test and say so explicitly."
    );
}
