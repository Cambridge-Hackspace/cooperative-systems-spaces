//! The two training-session handlers gate on deliberately different rules.
//!
//! An automated scan read `complete_training_session`, saw `Path(target_user_id)`
//! flow into a database write with no `user.0.id != target_user_id` comparison,
//! and reported an IDOR with a suggested fix: copy the ownership check from
//! `start_training_session`.
//!
//! It is a false positive, and the suggested fix would be a regression. The
//! handler does gate on
//! `can_access_staff() || is_certified_instructor(user, payload.training_step_id)`,
//! which is both stronger and correctly scoped. Requiring
//! `user.0.id == target_user_id` for non-staff would mean a certified
//! instructor could only complete their *own* training and could no longer
//! sign off a trainee, which is the entire purpose of the endpoint. It would
//! turn the one thing you would actually want to forbid -- self-certification
//! -- into the only thing permitted.
//!
//! The scan was right that the asymmetry is invisible, so this records it:
//!
//!   start:    your own, or staff acting for someone else.
//!   complete: staff, an instructor certified for *this step*, or the subject
//!             themselves on a step marked self-attestable.
//!
//! The third completion gate is issue #2 -- a binding "I have read this safety
//! documentation" confirmation -- and it is the answer to the question the
//! previous version of this file left open, which was whether somebody may
//! ever complete a step for themselves. The answer is: only on a step somebody
//! with staff rights marked self-attestable, only for themselves, never
//! carrying a score, and never on a step that requires an assessment. That last
//! constraint is enforced at the step editor rather than here, which is why
//! `reject_self_attestable_assessment` is pinned in this file too: without it
//! the completion gate's narrowness is a promise nothing keeps.
//!
//! Both rules are pinned so that removing any gate, or "fixing" one into
//! another, fails. A text-level check on purpose: it needs no database and no
//! compiler, so it runs on the FreeBSD workstation where `css-server` cannot be
//! built at all.
//!
//! What this does NOT prove: that the gates are correct at runtime -- only that
//! they are present, that the instructor check is scoped to the step being
//! completed rather than to instructor status in general, and that
//! self-attestation is scoped to the caller's own training. The route x
//! credential matrix in the contract tier covers reachability.

use css_checks::read;

fn training_source() -> String {
    read("server/src/api/training.rs")
}

/// The body of a named `async fn`, from its signature to the next line that is
/// a lone `}` at column zero, with line comments stripped.
///
/// Stripping is not tidiness. This file asserts the *absence* of
/// `user.0.id != target_user_id` from one handler, and the prose above the gate
/// it guards discusses that very comparison -- so an unstripped scan would read
/// the explanation as the offence. The mirror case is worse: a presence
/// assertion satisfied by a comment mentioning the call it looks for. Same
/// reasoning as route_parity.rs:79 and cli_api_paths.rs:37.
fn handler_body(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("async fn {name}("))
        .unwrap_or_else(|| panic!("no handler named `{name}` in server/src/api/training.rs"));
    let rest = &source[start..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    rest[..end]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn both_handlers_were_actually_found() {
    // Anti-vacuity. A rename would make every assertion below run over an
    // empty string and pass, which is the shape this whole file exists to
    // stop somewhere else.
    let source = training_source();
    for name in ["start_training_session", "complete_training_session"] {
        let body = handler_body(&source, name);
        assert!(
            body.len() > 200,
            "`{name}` parsed as {} bytes, which is too short to be the handler",
            body.len()
        );
        assert!(
            body.contains("Path(target_user_id)"),
            "`{name}` no longer takes the subject from the path. If the \
             signature changed, the rules recorded in this file have to be \
             re-derived rather than re-pointed."
        );
    }
}

#[test]
fn starting_a_session_is_your_own_or_staff() {
    let body = handler_body(&training_source(), "start_training_session");

    assert!(
        body.contains("user.0.id != target_user_id"),
        "`start_training_session` no longer compares the caller to the subject, \
         so any authenticated user can start training on anyone's behalf."
    );
    assert!(
        body.contains("can_access_staff()"),
        "`start_training_session` no longer checks for staff, so the \
         instructor-led case is either broken or ungated."
    );
    assert!(
        body.contains("ApiError::Forbidden"),
        "`start_training_session` has a comparison but no refusal, so the \
         check computes an answer nothing acts on."
    );
}

#[test]
fn completing_a_session_is_staff_or_an_instructor_for_that_step() {
    let body = handler_body(&training_source(), "complete_training_session");

    assert!(
        body.contains("can_access_staff()"),
        "`complete_training_session` no longer checks for staff."
    );
    assert!(
        body.contains("is_certified_instructor(user.0.id, payload.training_step_id)"),
        "`complete_training_session` no longer checks instructor certification \
         for the step in the payload.\n\n\
         Scope is the point: `is_certified_instructor(user, step)` means an \
         instructor may only sign off the step they are certified for. \
         Checking instructor status in general -- or checking it against a \
         different step than the one being completed -- would let anyone \
         certified for any step complete any other."
    );
    assert!(
        body.contains("ApiError::Forbidden"),
        "`complete_training_session` computes `can_complete` and no longer \
         refuses on it."
    );
}

#[test]
fn completing_your_own_is_permitted_only_on_a_self_attestable_step() {
    let body = handler_body(&training_source(), "complete_training_session");

    assert!(
        body.contains("step.self_attestable && user.0.id == target_user_id"),
        "the self-attestation gate is gone or has changed shape.\n\n\
         Both halves are load-bearing and neither is sufficient alone. \
         `step.self_attestable` keeps this off every step by default, so it is \
         staff who decide which steps a member may confirm. \
         `user.0.id == target_user_id` keeps it to the caller's own training -- \
         without it, a self-attestable step would be completable by any \
         authenticated user for any other user, which is a far worse hole than \
         the one this feature was added to close."
    );
    assert!(
        body.contains("|| is_self_attestation"),
        "`is_self_attestation` is computed but no longer widens `can_complete`, \
         so the self-service path is dead code and the checkbox 403s."
    );
}

#[test]
fn a_self_attestation_carries_no_score_and_cannot_be_a_failure() {
    let body = handler_body(&training_source(), "complete_training_session");

    assert!(
        body.contains("payload.assessment_score.is_some()"),
        "a self-attestation may once again carry an assessment score, so a \
         member can post themselves a mark on their own training record."
    );
    assert!(
        body.contains("if !payload.passed"),
        "a self-attestation may once again be recorded as not passed. That is \
         refused rather than overridden on purpose: quietly rewriting a \
         caller's field is how a record ends up asserting something nobody \
         submitted."
    );
    assert!(
        body.matches("ApiError::BadRequest").count() >= 2,
        "the two attestation guards no longer both refuse. Computing a \
         condition nothing acts on is the failure mode this asserts against."
    );
}

#[test]
fn a_self_attestable_step_can_never_require_an_assessment() {
    // The completion gate is only as narrow as this validator makes it. If a
    // step could be both, a member confirming they read something would be
    // recording that they passed a practical assessment.
    let source = training_source();

    assert!(
        source.contains("fn reject_self_attestable_assessment("),
        "`reject_self_attestable_assessment` is gone. Without it a step can be \
         both self-attestable and assessed, and the completion gate's \
         narrowness -- which this file asserts -- becomes a promise nothing \
         keeps."
    );

    for handler in ["create_training_step", "update_training_step"] {
        let body = handler_body(&source, handler);
        assert!(
            body.contains("reject_self_attestable_assessment("),
            "`{handler}` no longer rejects a step that is both self-attestable \
             and assessed, so one can be written through this route."
        );
    }

    // The update path has to check the row as it will be, not as the request
    // describes it: both fields are Option and None means "leave alone", so a
    // request carrying only `self_attestable: true` says nothing about
    // assessment. Checking the payload alone would let it land on a step that
    // already requires one.
    let update = handler_body(&source, "update_training_step");
    assert!(
        update.contains("unwrap_or(existing.self_attestable)")
            && update.contains("unwrap_or(existing.requires_assessment)"),
        "`update_training_step` validates the payload rather than the effective \
         values. A request that sets only `self_attestable` would then pass \
         while landing on a step that already requires an assessment."
    );
}

#[test]
fn completing_did_not_acquire_an_ownership_requirement() {
    // The suggested "fix" from the original scan, pinned as a regression rather
    // than left to be rediscovered and applied by the next one.
    //
    // Note the asymmetry with the gate asserted above: `==` widens (one more
    // way to be allowed), `!=` narrows (a condition everyone must satisfy).
    // Self-attestation adds the first. The second would still forbid the
    // instructor-led sign-off this endpoint exists for.
    let body = handler_body(&training_source(), "complete_training_session");

    assert!(
        !body.contains("user.0.id != target_user_id"),
        "`complete_training_session` gained an ownership *requirement*.\n\n\
         If this was added to answer an IDOR report, it is the wrong fix: it \
         restricts a non-staff instructor to completing their own training, \
         which forbids the instructor-led sign-off this endpoint exists for and \
         permits the self-certification it should not. The staff-or-certified- \
         instructor gate is the stronger check, and self-attestation is already \
         expressed as `==` widening it rather than `!=` narrowing it.\n\n\
         If it was added deliberately for some other reason, delete this test \
         and say why in the commit message."
    );
}
