//! Statement validation, the per-session sequence machine, and moveOn evaluation.
//!
//! This is the pure half of the security boundary. The server binds a session to
//! exactly one learner, registration, and activity, then hands each incoming
//! statement here with that [`SessionExpectation`]. Every check that decides
//! whether a statement is allowed to count — and therefore whether it can lead to
//! a physical-tool-access grant — lives in [`validate_cmi5_statement`] and
//! [`SessionState::apply`]. Nothing here reads a database or trusts a field the
//! content chose for "who" or "which activity": those come from the expectation,
//! which the server derived from the authenticated launch.

use crate::manifest::MoveOn;
use crate::statement::{Statement, categories, verbs};
use uuid::Uuid;

/// The server-side truth about a launched session. Every statement is judged
/// against this; none of it is taken from the statement itself.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionExpectation {
    /// The actor account `homePage` the LMS issued at launch.
    pub actor_home_page: String,
    /// The actor account `name` (the learner's user id) the LMS issued at launch.
    pub actor_account_name: String,
    /// The registration this session was launched with.
    pub registration: Uuid,
    /// The AU's activity IRI.
    pub activity_id: String,
    /// The AU's masteryScore, if any; a `passed` below it does not count.
    pub mastery_score: Option<f64>,
}

/// Why a statement was rejected. Naming the exact reason is what lets the server
/// answer precisely and lets tests assert the specific defense that fired rather
/// than merely that rejection happened.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Violation {
    #[error("actor is not an account-based agent")]
    NotAnAccountActor,
    #[error("actor does not match the launched learner")]
    ActorMismatch,
    #[error("statement registration does not match the session")]
    RegistrationMismatch,
    #[error("statement is about a different activity than the launched AU")]
    ActivityMismatch,
    #[error("statement is missing the cmi5 context category")]
    MissingCmi5Category,
    #[error("'{0}' is not a verb an AU may issue")]
    UnknownVerb(String),
    #[error("statement is missing the result its verb requires")]
    MissingResult,
    #[error("a 'passed' statement must have result.success = true")]
    NotPassedSuccess,
    #[error("a 'failed' statement must have result.success = false")]
    NotFailedSuccess,
    #[error("a 'completed' statement must have result.completion = true")]
    NotCompleted,
    #[error("score.scaled {scaled:?} is below the required masteryScore {required}")]
    BelowMasteryScore { scaled: Option<f64>, required: f64 },
    #[error("a statement arrived before 'initialized'")]
    NotInitialized,
    #[error("'initialized' arrived more than once")]
    AlreadyInitialized,
    #[error("a statement arrived after 'terminated'")]
    AfterTerminated,
    #[error("the session recorded both 'passed' and 'failed'")]
    BothPassedAndFailed,
}

/// Whether a statement carries a given context category activity.
fn has_category(stmt: &Statement, iri: &str) -> bool {
    stmt.context
        .as_ref()
        .and_then(|c| c.context_activities.as_ref())
        .map(|ca| ca.has_category(iri))
        .unwrap_or(false)
}

/// The AU-issued verbs the LRS accepts from content. (LMS-issued verbs such as
/// `launched` and `satisfied` are written by the server, not validated here.)
fn is_au_verb(verb: &str) -> bool {
    matches!(
        verb,
        verbs::INITIALIZED | verbs::COMPLETED | verbs::PASSED | verbs::FAILED | verbs::TERMINATED
    )
}

/// Validate a single content-issued statement against the session.
///
/// Order matters: identity and binding are checked before result/score, so a
/// forged statement fails as "wrong actor/activity" rather than leaking that its
/// score would have passed. Returns `Ok(())` for a statement that is allowed to
/// be stored and fed to the [`SessionState`].
pub fn validate_cmi5_statement(
    stmt: &Statement,
    expect: &SessionExpectation,
) -> Result<(), Violation> {
    // 1. Actor must be the launched learner, by account. This is the check that
    //    stops a learner posting a statement "as" someone else.
    let account = stmt
        .actor
        .account
        .as_ref()
        .ok_or(Violation::NotAnAccountActor)?;
    if account.home_page != expect.actor_home_page || account.name != expect.actor_account_name {
        return Err(Violation::ActorMismatch);
    }

    // 2. Registration must match the launched session.
    if stmt.registration() != Some(expect.registration) {
        return Err(Violation::RegistrationMismatch);
    }

    // 3. The statement must be about the launched AU — not some other AU whose
    //    step the learner would rather have credited.
    if stmt.object_activity_id() != expect.activity_id {
        return Err(Violation::ActivityMismatch);
    }

    // 4. It must be a cmi5-defined statement.
    if !has_category(stmt, categories::CMI5) {
        return Err(Violation::MissingCmi5Category);
    }

    // 5. Verb must be one an AU may issue.
    let verb = stmt.verb_id();
    if !is_au_verb(verb) {
        return Err(Violation::UnknownVerb(verb.to_string()));
    }

    // 6. Verb/result consistency, including the masteryScore gate on `passed`.
    match verb {
        verbs::PASSED => {
            let result = stmt.result.as_ref().ok_or(Violation::MissingResult)?;
            if result.success != Some(true) {
                return Err(Violation::NotPassedSuccess);
            }
            if let Some(required) = expect.mastery_score {
                let scaled = result.score.as_ref().and_then(|s| s.scaled);
                if scaled.map(|v| v < required).unwrap_or(true) {
                    return Err(Violation::BelowMasteryScore { scaled, required });
                }
            }
        }
        verbs::FAILED => {
            let result = stmt.result.as_ref().ok_or(Violation::MissingResult)?;
            if result.success != Some(false) {
                return Err(Violation::NotFailedSuccess);
            }
        }
        verbs::COMPLETED => {
            let result = stmt.result.as_ref().ok_or(Violation::MissingResult)?;
            if result.completion != Some(true) {
                return Err(Violation::NotCompleted);
            }
        }
        _ => {}
    }

    Ok(())
}

/// The satisfaction-relevant outcomes observed in a session so far. Only
/// statements carrying the moveon category set these — a `passed` that is not a
/// moveOn statement does not move the learner on, per cmi5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionOutcome {
    pub passed: bool,
    pub completed: bool,
    pub failed: bool,
}

/// The per-session sequence machine. Fed validated statements in arrival order,
/// it enforces cmi5's ordering rules and accumulates the [`SessionOutcome`] that
/// [`evaluate_move_on`] reads.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    initialized: bool,
    terminated: bool,
    outcome: SessionOutcome,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn outcome(&self) -> SessionOutcome {
        self.outcome
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    /// Apply a statement that has already passed [`validate_cmi5_statement`].
    ///
    /// Enforces: nothing after `terminated`; nothing before `initialized`;
    /// `initialized` at most once; not both `passed` and `failed`. Records the
    /// outcome only for statements that carry the moveon category.
    pub fn apply(&mut self, stmt: &Statement) -> Result<(), Violation> {
        if self.terminated {
            return Err(Violation::AfterTerminated);
        }
        let verb = stmt.verb_id();
        let counts = stmt.is_moveon();

        match verb {
            verbs::INITIALIZED => {
                if self.initialized {
                    return Err(Violation::AlreadyInitialized);
                }
                self.initialized = true;
            }
            verbs::PASSED => {
                if !self.initialized {
                    return Err(Violation::NotInitialized);
                }
                if counts {
                    if self.outcome.failed {
                        return Err(Violation::BothPassedAndFailed);
                    }
                    self.outcome.passed = true;
                }
            }
            verbs::FAILED => {
                if !self.initialized {
                    return Err(Violation::NotInitialized);
                }
                if counts {
                    if self.outcome.passed {
                        return Err(Violation::BothPassedAndFailed);
                    }
                    self.outcome.failed = true;
                }
            }
            verbs::COMPLETED => {
                if !self.initialized {
                    return Err(Violation::NotInitialized);
                }
                if counts {
                    self.outcome.completed = true;
                }
            }
            verbs::TERMINATED => {
                if !self.initialized {
                    return Err(Violation::NotInitialized);
                }
                self.terminated = true;
            }
            other => return Err(Violation::UnknownVerb(other.to_string())),
        }
        Ok(())
    }
}

/// Decide whether an AU's moveOn criterion is met by the observed outcome.
///
/// `NotApplicable` never satisfies here: an AU with no moveOn criterion cannot
/// auto-grant tool access, so mapping such an AU to a gating step fails closed
/// rather than opening a machine on ambiguous evidence.
pub fn evaluate_move_on(move_on: MoveOn, outcome: &SessionOutcome) -> bool {
    match move_on {
        MoveOn::Passed => outcome.passed,
        MoveOn::Completed => outcome.completed,
        MoveOn::CompletedAndPassed => outcome.completed && outcome.passed,
        MoveOn::CompletedOrPassed => outcome.completed || outcome.passed,
        MoveOn::NotApplicable => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statement::{
        Account, Activity, Agent, Context, ContextActivities, Score, Statement, StatementObject,
        Verb, XResult,
    };

    fn expectation() -> SessionExpectation {
        SessionExpectation {
            actor_home_page: "https://space.example".to_string(),
            actor_account_name: "learner-1".to_string(),
            registration: Uuid::from_u128(0xAA),
            activity_id: "http://example.com/au/1".to_string(),
            mastery_score: Some(0.8),
        }
    }

    /// Build a statement with the session's identity by default; callers mutate
    /// the one field a given test is about, so each test isolates one rule.
    fn stmt(verb: &str) -> Statement {
        Statement {
            id: Some(Uuid::from_u128(1)),
            actor: Agent {
                object_type: Some("Agent".into()),
                name: None,
                mbox: None,
                account: Some(Account {
                    home_page: "https://space.example".into(),
                    name: "learner-1".into(),
                }),
            },
            verb: Verb {
                id: verb.to_string(),
                display: None,
            },
            object: StatementObject::Activity(Activity {
                object_type: Some("Activity".into()),
                id: "http://example.com/au/1".into(),
                definition: None,
            }),
            result: None,
            context: Some(Context {
                registration: Some(Uuid::from_u128(0xAA)),
                context_activities: Some(ContextActivities {
                    category: Some(vec![
                        Activity {
                            object_type: None,
                            id: categories::CMI5.into(),
                            definition: None,
                        },
                        Activity {
                            object_type: None,
                            id: categories::MOVEON.into(),
                            definition: None,
                        },
                    ]),
                    parent: None,
                    grouping: None,
                    other: None,
                }),
                extensions: None,
            }),
            timestamp: None,
        }
    }

    fn passed(scaled: f64) -> Statement {
        let mut s = stmt(verbs::PASSED);
        s.result = Some(XResult {
            success: Some(true),
            score: Some(Score {
                scaled: Some(scaled),
                ..Default::default()
            }),
            ..Default::default()
        });
        s
    }

    #[test]
    fn accepts_a_well_formed_passed() {
        assert_eq!(
            validate_cmi5_statement(&passed(0.9), &expectation()),
            Ok(())
        );
    }

    #[test]
    fn rejects_a_foreign_actor() {
        let mut s = passed(0.9);
        s.actor.account.as_mut().unwrap().name = "someone-else".into();
        assert_eq!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::ActorMismatch)
        );
    }

    #[test]
    fn rejects_a_wrong_registration() {
        let mut s = passed(0.9);
        s.context.as_mut().unwrap().registration = Some(Uuid::from_u128(0xBB));
        assert_eq!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::RegistrationMismatch)
        );
    }

    #[test]
    fn rejects_a_pass_for_another_activity() {
        let mut s = passed(0.9);
        s.object = StatementObject::Activity(Activity {
            object_type: None,
            id: "http://example.com/au/OTHER".into(),
            definition: None,
        });
        assert_eq!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::ActivityMismatch)
        );
    }

    #[test]
    fn rejects_a_pass_below_mastery() {
        let s = passed(0.5);
        assert_eq!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::BelowMasteryScore {
                scaled: Some(0.5),
                required: 0.8
            })
        );
    }

    #[test]
    fn rejects_a_pass_with_no_score_when_mastery_is_required() {
        let mut s = passed(0.9);
        s.result.as_mut().unwrap().score = None;
        assert_eq!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::BelowMasteryScore {
                scaled: None,
                required: 0.8
            })
        );
    }

    #[test]
    fn rejects_a_missing_cmi5_category() {
        let mut s = passed(0.9);
        s.context.as_mut().unwrap().context_activities = None;
        assert_eq!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::MissingCmi5Category)
        );
    }

    #[test]
    fn rejects_an_unknown_verb() {
        let mut s = passed(0.9);
        s.verb.id = verbs::SATISFIED.to_string(); // LMS-only verb, not AU-issuable
        assert!(matches!(
            validate_cmi5_statement(&s, &expectation()),
            Err(Violation::UnknownVerb(_))
        ));
    }

    #[test]
    fn sequence_requires_initialized_first() {
        let mut state = SessionState::new();
        assert_eq!(state.apply(&passed(0.9)), Err(Violation::NotInitialized));
    }

    #[test]
    fn sequence_rejects_statements_after_terminated() {
        let mut state = SessionState::new();
        state.apply(&stmt(verbs::INITIALIZED)).unwrap();
        state.apply(&stmt(verbs::TERMINATED)).unwrap();
        assert_eq!(state.apply(&passed(0.9)), Err(Violation::AfterTerminated));
    }

    #[test]
    fn sequence_rejects_both_passed_and_failed() {
        let mut state = SessionState::new();
        state.apply(&stmt(verbs::INITIALIZED)).unwrap();
        state.apply(&passed(0.9)).unwrap();
        let mut fail = stmt(verbs::FAILED);
        fail.result = Some(XResult {
            success: Some(false),
            ..Default::default()
        });
        assert_eq!(state.apply(&fail), Err(Violation::BothPassedAndFailed));
    }

    #[test]
    fn a_non_moveon_pass_does_not_count() {
        let mut state = SessionState::new();
        state.apply(&stmt(verbs::INITIALIZED)).unwrap();
        let mut s = passed(0.9);
        // Strip the moveon category: still a valid cmi5 statement, but it must
        // not move the learner on.
        s.context
            .as_mut()
            .unwrap()
            .context_activities
            .as_mut()
            .unwrap()
            .category = Some(vec![Activity {
            object_type: None,
            id: categories::CMI5.into(),
            definition: None,
        }]);
        state.apply(&s).unwrap();
        assert_eq!(state.outcome(), SessionOutcome::default());
    }

    #[test]
    fn move_on_truth_table() {
        let cases = [
            (MoveOn::Passed, true, false, true),
            (MoveOn::Passed, false, true, false),
            (MoveOn::Completed, false, true, true),
            (MoveOn::Completed, true, false, false),
            (MoveOn::CompletedAndPassed, true, true, true),
            (MoveOn::CompletedAndPassed, true, false, false),
            (MoveOn::CompletedAndPassed, false, true, false),
            (MoveOn::CompletedOrPassed, true, false, true),
            (MoveOn::CompletedOrPassed, false, true, true),
            (MoveOn::CompletedOrPassed, false, false, false),
            (MoveOn::NotApplicable, true, true, false),
        ];
        for (move_on, passed, completed, expected) in cases {
            let outcome = SessionOutcome {
                passed,
                completed,
                failed: false,
            };
            assert_eq!(
                evaluate_move_on(move_on, &outcome),
                expected,
                "moveOn={move_on:?} passed={passed} completed={completed}"
            );
        }
    }
}
