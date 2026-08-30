//! The two training-session handlers gate on deliberately different rules.
//!
//! An automated scan read `complete_training_session`, saw `Path(target_user_id)`
//! flow into a database write with no `user.0.id != target_user_id` comparison,
//! and reported an IDOR with a suggested fix: copy the ownership check from
//! `start_training_session`.
//!
//! It is a false positive, and the suggested fix would be a regression. The
//! handler does gate -- three lines above the write -- on
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
//!   complete: staff, or an instructor certified for *this step*.
//!
//! Both rules are pinned here so that removing either gate, or "fixing" one
//! into the other, fails. A text-level check on purpose: it needs no database
//! and no compiler, so it runs on the FreeBSD workstation where `css-server`
//! cannot be built at all.
//!
//! What this does NOT prove: that the gates are correct at runtime -- only that
//! they are present, and that the instructor check is scoped to the step being
//! completed rather than to instructor status in general. The route x
//! credential matrix in the contract tier covers reachability; neither covers
//! an instructor completing a step for themselves, which is a live question
//! nobody has answered.

use css_checks::read;

fn training_source() -> String {
    read("server/src/api/training.rs")
}

/// The body of a named `async fn`, from its signature to the next line that is
/// a lone `}` at column zero.
fn handler_body(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("async fn {name}("))
        .unwrap_or_else(|| panic!("no handler named `{name}` in server/src/api/training.rs"));
    let rest = &source[start..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    rest[..end].to_string()
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
fn completing_did_not_acquire_an_ownership_check() {
    // The suggested "fix", pinned as a regression rather than left to be
    // rediscovered and applied by the next scan.
    let body = handler_body(&training_source(), "complete_training_session");

    assert!(
        !body.contains("user.0.id != target_user_id"),
        "`complete_training_session` gained an ownership comparison.\n\n\
         If this was added to answer an IDOR report, it is the wrong fix: it \
         restricts a non-staff instructor to completing their *own* training, \
         which forbids the instructor-led sign-off this endpoint exists for \
         and permits the self-certification it should not. The existing \
         staff-or-certified-instructor gate is the stronger check.\n\n\
         If it was added deliberately for some other reason, delete this test \
         and say why in the commit message."
    );
}
