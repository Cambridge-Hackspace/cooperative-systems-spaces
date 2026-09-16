//! Bypass-detection decisions (#84), kept free of I/O so they can be tested
//! directly.
//!
//! Two rules live here, and both exist to keep an unbounded audit table
//! readable. Nothing prunes `audit_logs` -- there is no retention at all -- so a
//! detector that logs a *level* ("this module is still silent") rather than a
//! *transition* ("this module went silent at T") writes one row per sweep
//! forever and buries the signal in itself. Every function below answers
//! "did something change?", never "what is the current state?".
//!
//! Deliberately dependency-free: durations are plain seconds rather than
//! `chrono` types, so this crate stays at serde and the callers keep their own
//! clocks.

use serde::{Deserialize, Serialize};

/// What, if anything, to record about a module's liveness this sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LivenessTransition {
    /// It has stopped reporting and we had not said so yet.
    WentSilent,
    /// It is reporting again after we had said it was silent.
    Returned,
    /// Nothing worth a row.
    NoChange,
}

/// Decide whether a bound module's liveness changed.
///
/// `seconds_since_seen` is `None` when the device has never reported at all.
/// That counts as silent: a module that was bound to a tool and has never once
/// checked in is not reporting, and saying so promptly is the point -- it is
/// usually a misconfiguration somebody wants to hear about the same afternoon,
/// not a fault that shows up when a member is standing at the machine.
///
/// `previously_reported_silent` is the state we last *recorded*, not the state
/// we last observed. Deriving it from the audit trail is what makes this
/// survive a restart without double-reporting.
pub fn classify_liveness(
    seconds_since_seen: Option<i64>,
    threshold_secs: i64,
    previously_reported_silent: bool,
) -> LivenessTransition {
    let silent = match seconds_since_seen {
        None => true,
        Some(secs) => secs > threshold_secs,
    };
    match (silent, previously_reported_silent) {
        (true, false) => LivenessTransition::WentSilent,
        (false, true) => LivenessTransition::Returned,
        _ => LivenessTransition::NoChange,
    }
}

/// The evidence that a tool drew power it was not authorized to draw.
///
/// Two independent oracles, because they fail differently. A plug that
/// misreports its relay is caught by draw; a tool drawing through a plug that
/// was bypassed entirely is caught by draw alone; a relay closed while the tool
/// idles is caught by state alone. Recording which one fired is what makes the
/// row worth anything afterwards -- "the plug said it was on" and "the circuit
/// was pulling six amps" are different claims and an insurer will ask which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PowerEvidence {
    /// The module reported its relay closed.
    pub relay_reported_on: bool,
    /// Measured draw exceeded the idle threshold.
    pub draw_observed: bool,
}

impl PowerEvidence {
    pub fn any(&self) -> bool {
        self.relay_reported_on || self.draw_observed
    }
}

/// Whether an observation of a tool being powered, with no lease authorizing
/// it, should be recorded as unauthorized.
///
/// `seconds_observed` debounces: a relay stays closed for a moment after a
/// legitimate switch-off, and logging that as a bypass would train everyone to
/// ignore the event. Nothing here acts on the finding -- draw on a circuit is
/// not proof that *this* tool ran, and a detection signal is not grounds to cut
/// power to a machine somebody may be holding.
pub fn is_unauthorized_power(
    evidence: PowerEvidence,
    lease_valid: bool,
    seconds_observed: i64,
    debounce_secs: i64,
) -> bool {
    if lease_valid || !evidence.any() {
        return false;
    }
    seconds_observed >= debounce_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: i64 = 90;

    #[test]
    fn a_module_reporting_inside_the_threshold_is_quiet_news() {
        assert_eq!(
            classify_liveness(Some(10), THRESHOLD, false),
            LivenessTransition::NoChange
        );
    }

    #[test]
    fn crossing_the_threshold_reports_once_and_only_once() {
        assert_eq!(
            classify_liveness(Some(91), THRESHOLD, false),
            LivenessTransition::WentSilent
        );
        // The next sweep, still silent, must not write another row. This is the
        // whole reason the decision takes the previously-recorded state.
        assert_eq!(
            classify_liveness(Some(6000), THRESHOLD, true),
            LivenessTransition::NoChange
        );
    }

    #[test]
    fn coming_back_reports_once_and_only_once() {
        assert_eq!(
            classify_liveness(Some(5), THRESHOLD, true),
            LivenessTransition::Returned
        );
        assert_eq!(
            classify_liveness(Some(5), THRESHOLD, false),
            LivenessTransition::NoChange
        );
    }

    #[test]
    fn exactly_at_the_threshold_is_not_yet_silent() {
        // Strictly greater, so a module reporting on a perfectly punctual
        // keep-alive boundary does not flap between the two states.
        assert_eq!(
            classify_liveness(Some(THRESHOLD), THRESHOLD, false),
            LivenessTransition::NoChange
        );
    }

    #[test]
    fn a_module_that_has_never_reported_is_silent() {
        assert_eq!(
            classify_liveness(None, THRESHOLD, false),
            LivenessTransition::WentSilent
        );
        assert_eq!(
            classify_liveness(None, THRESHOLD, true),
            LivenessTransition::NoChange
        );
    }

    #[test]
    fn a_valid_lease_means_the_power_is_authorized_whatever_the_evidence() {
        let both = PowerEvidence {
            relay_reported_on: true,
            draw_observed: true,
        };
        assert!(!is_unauthorized_power(both, true, 3600, 5));
    }

    #[test]
    fn no_evidence_is_not_a_finding() {
        assert!(!is_unauthorized_power(
            PowerEvidence::default(),
            false,
            3600,
            5
        ));
    }

    #[test]
    fn either_oracle_alone_is_enough() {
        let relay = PowerEvidence {
            relay_reported_on: true,
            draw_observed: false,
        };
        let draw = PowerEvidence {
            relay_reported_on: false,
            draw_observed: true,
        };
        assert!(is_unauthorized_power(relay, false, 10, 5));
        assert!(is_unauthorized_power(draw, false, 10, 5));
    }

    #[test]
    fn a_brief_observation_is_debounced_away() {
        let relay = PowerEvidence {
            relay_reported_on: true,
            draw_observed: false,
        };
        // A relay still closed a second after a legitimate switch-off.
        assert!(!is_unauthorized_power(relay, false, 1, 5));
        // Still closed well past the debounce: now it is a finding.
        assert!(is_unauthorized_power(relay, false, 5, 5));
    }
}
