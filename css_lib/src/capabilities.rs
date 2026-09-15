//! What a physical tool module can actually do, and what that permits (#83).
//!
//! An interlock's `enforcement` tier is not a free choice. A rule can only be
//! enforced in firmware if some single module can *both* sense the condition and
//! act on it locally; otherwise the trip has to travel through the edge, however
//! much one might prefer it did not. Declaring the tier without checking it
//! against the hardware produces a rule that silently does not do what it says,
//! which for a safety interlock is the worst of the available outcomes.
//!
//! Capabilities are declared per binding, under `tool_modules.params`:
//!
//! ```json
//! {
//!   "capabilities": {
//!     "local_inputs": ["door_open"],
//!     "local_inhibit": true,
//!     "countdown": false,
//!     "holds_last_on_disconnect": false
//!   }
//! }
//! ```
//!
//! Everything defaults to "cannot": an undeclared module is assumed to do
//! nothing locally, so an unverified plug can never be the reason a `firmware`
//! tier is accepted. Filling in the real descriptors for the hardware in use is
//! a separate inventory task; this is the shape they take and the rule they feed.

use serde::{Deserialize, Serialize};

/// A module's declared local abilities. Absent fields mean "cannot".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleCapabilities {
    /// Conditions this module senses directly -- wired to it, not relayed.
    #[serde(default)]
    pub local_inputs: Vec<String>,
    /// Whether it can cut its own output without being told to.
    #[serde(default)]
    pub local_inhibit: bool,
    /// Whether it supports an auto-off countdown that a lease can ride on
    /// (Zigbee's on-with-timed-off and equivalents). This is what lets an
    /// unmodifiable plug still fail safe: the coordinator re-arms the countdown
    /// as its renewal, so silence expires the relay without any firmware change.
    #[serde(default)]
    pub countdown: bool,
    /// Whether it keeps its last relay state when its link drops. A plug that
    /// holds last and has neither `local_inhibit` nor `countdown` cannot fail
    /// safe on its own at all.
    #[serde(default)]
    pub holds_last_on_disconnect: bool,
}

impl ModuleCapabilities {
    /// Read a binding's declared capabilities out of its `params` blob.
    ///
    /// A missing or malformed `capabilities` object yields the all-cannot
    /// default rather than an error: the caller's question is "may this module
    /// be relied on", and the answer for something undeclared is no.
    pub fn from_params(params: &serde_json::Value) -> Self {
        params
            .get("capabilities")
            .and_then(|c| serde_json::from_value::<Self>(c.clone()).ok())
            .unwrap_or_default()
    }

    /// Whether this module alone can enforce `condition` in firmware: it senses
    /// the condition directly *and* can act on it without being told.
    pub fn can_enforce_locally(&self, condition: &str) -> bool {
        self.local_inhibit && self.local_inputs.iter().any(|c| c == condition)
    }

    /// Whether this module reaches a safe state on its own when it stops hearing
    /// from the coordinator. Either it can inhibit itself, or it has a countdown
    /// the lease can ride, or it simply does not hold its last state.
    pub fn can_fail_safe(&self) -> bool {
        self.local_inhibit || self.countdown || !self.holds_last_on_disconnect
    }
}

/// Why a requested enforcement tier is not achievable with the bound hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAchievable {
    pub reason: String,
}

/// Whether `firmware` enforcement of `condition` is achievable given the
/// capabilities of the modules bound to the tool.
///
/// The requirement is deliberately that *one* module does both jobs. Two modules
/// that between them sense and act still have to be connected by something, and
/// that something is the edge -- which is the `edge` tier, not this one.
pub fn firmware_enforcement_achievable(
    condition: &str,
    modules: &[ModuleCapabilities],
) -> Result<(), NotAchievable> {
    if modules.iter().any(|m| m.can_enforce_locally(condition)) {
        return Ok(());
    }
    Err(NotAchievable {
        reason: format!(
            "no module bound to this tool both senses '{condition}' locally and can inhibit its \
             own output, so this interlock cannot be enforced in firmware. Wire the sensor \
             directly to the module that switches the tool, or set enforcement to 'edge'."
        ),
    })
}

/// Whether any power module bound to this tool can reach a safe state on its own.
///
/// Not an error -- plenty of tools are wired to an unmodifiable plug that holds
/// its last state, and the honest answer there is that the coordinator's cut is a
/// mitigation rather than an interlock. The caller surfaces it so the gap is
/// visible rather than assumed away.
pub fn power_can_fail_safe(power_modules: &[ModuleCapabilities]) -> bool {
    !power_modules.is_empty() && power_modules.iter().all(|m| m.can_fail_safe())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn caps(
        inputs: &[&str],
        inhibit: bool,
        countdown: bool,
        holds_last: bool,
    ) -> ModuleCapabilities {
        ModuleCapabilities {
            local_inputs: inputs.iter().map(|s| s.to_string()).collect(),
            local_inhibit: inhibit,
            countdown,
            holds_last_on_disconnect: holds_last,
        }
    }

    #[test]
    fn an_undeclared_module_can_do_nothing() {
        let c = ModuleCapabilities::from_params(&json!({}));
        assert_eq!(c, ModuleCapabilities::default());
        assert!(!c.can_enforce_locally("door_open"));
        // Nothing declared means it does not claim to hold last either, so it is
        // not assumed to strand a tool energized.
        assert!(c.can_fail_safe());
    }

    #[test]
    fn malformed_capabilities_do_not_grant_anything() {
        // A typo in the blob must not read as "yes it can".
        let c = ModuleCapabilities::from_params(&json!({ "capabilities": "definitely" }));
        assert!(!c.local_inhibit);
        assert!(!c.can_enforce_locally("door_open"));
    }

    #[test]
    fn capabilities_round_trip_from_params() {
        let c = ModuleCapabilities::from_params(&json!({
            "capabilities": {
                "local_inputs": ["door_open", "estop"],
                "local_inhibit": true,
                "countdown": false,
                "holds_last_on_disconnect": true
            }
        }));
        assert!(c.can_enforce_locally("door_open"));
        assert!(c.can_enforce_locally("estop"));
        assert!(!c.can_enforce_locally("lid_open"));
    }

    #[test]
    fn firmware_enforcement_needs_one_module_to_do_both_jobs() {
        // Senses the door but cannot switch: not achievable.
        let sensor_only = caps(&["door_open"], false, false, false);
        // Switches but senses nothing: not achievable.
        let plug_only = caps(&[], true, false, true);
        assert!(firmware_enforcement_achievable("door_open", &[sensor_only.clone()]).is_err());
        assert!(firmware_enforcement_achievable("door_open", &[plug_only.clone()]).is_err());
        // Two modules that between them could do it still cannot: connecting
        // them is the edge's job, which is a different tier.
        assert!(firmware_enforcement_achievable("door_open", &[sensor_only, plug_only]).is_err());

        // One module wired to the reed and able to cut its own relay: achievable.
        let integrated = caps(&["door_open"], true, false, false);
        assert!(firmware_enforcement_achievable("door_open", &[integrated]).is_ok());
    }

    #[test]
    fn the_refusal_says_what_to_do_about_it() {
        let err = firmware_enforcement_achievable("door_open", &[]).unwrap_err();
        assert!(err.reason.contains("door_open"));
        assert!(
            err.reason.contains("edge"),
            "a refusal that does not name the achievable alternative just blocks somebody"
        );
    }

    #[test]
    fn an_unmodifiable_plug_that_holds_last_cannot_fail_safe() {
        // The worked example: a Zigbee plug we cannot reflash, no local safety
        // input, and it keeps its relay state when the link drops.
        let dumb_plug = caps(&[], false, false, true);
        assert!(!dumb_plug.can_fail_safe());
        assert!(!power_can_fail_safe(&[dumb_plug]));
    }

    #[test]
    fn a_countdown_reclaims_fail_safe_without_touching_firmware() {
        // Same plug, but it supports on-with-timed-off: the coordinator re-arms
        // the countdown as the lease renewal, so silence expires the relay.
        let countdown_plug = caps(&[], false, true, true);
        assert!(countdown_plug.can_fail_safe());
        assert!(power_can_fail_safe(&[countdown_plug]));
    }

    #[test]
    fn a_tool_with_no_power_module_is_not_reported_as_fail_safe() {
        // Vacuous truth would say "all zero of them can fail safe". That would
        // read as reassurance about a tool nothing is even switching.
        assert!(!power_can_fail_safe(&[]));
    }

    #[test]
    fn one_unsafe_plug_among_several_is_still_unsafe() {
        let good = caps(&[], true, false, false);
        let bad = caps(&[], false, false, true);
        assert!(!power_can_fail_safe(&[good, bad]));
    }
}
