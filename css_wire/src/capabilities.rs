//! The capability descriptors a device declares at registration (#83, #101,
//! #209): what it can do locally, and so whether it can fail safe.
//!
//! Only the shape and the few rules that belong to it live here. What the
//! server does with them -- whether an interlock's `firmware` tier is
//! achievable, whether a tool's power fails safe -- is decided in
//! `css_lib::capabilities`, which re-exports this type.

use serde::{Deserialize, Serialize};

/// A module's declared local abilities. Absent fields mean the conservative
/// answer -- "cannot", or for `holds_last_on_disconnect`, "does".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// safe on its own at all -- which is why the DEFAULT is `true`: a device
    /// that has not said is assumed to be that plug, not its opposite.
    #[serde(default = "assume_holds_last")]
    pub holds_last_on_disconnect: bool,
}

/// The conservative default for `holds_last_on_disconnect`. A function rather
/// than `#[serde(default)]` because serde's default for a bool is `false`,
/// which here is the OPTIMISTIC answer.
fn assume_holds_last() -> bool {
    true
}

impl Default for ModuleCapabilities {
    /// Hand-written so that `from_value`'s `unwrap_or_default()` and serde's
    /// per-field defaults agree: both say "cannot, and holds last".
    fn default() -> Self {
        ModuleCapabilities {
            local_inputs: Vec::new(),
            local_inhibit: false,
            countdown: false,
            holds_last_on_disconnect: true,
        }
    }
}

impl ModuleCapabilities {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The declared form, as a device sends it at registration. Before #219 this
    /// literal was pinned verbatim on both sides of the repository boundary,
    /// because spaces-device had its own copy of these four names; now that it
    /// serialises this type, the literal pins the one definition against a
    /// rename that would quietly turn every declared "can" into a default
    /// "cannot".
    #[test]
    fn the_firmware_crates_declared_form_means_what_it_says_here() {
        let caps: ModuleCapabilities = serde_json::from_value(serde_json::json!({
            "roles": ["power"],
            "local_inputs": ["door_open"],
            "local_inhibit": true,
            "countdown": false,
            "holds_last_on_disconnect": true
        }))
        .unwrap();
        assert_eq!(caps.local_inputs, vec!["door_open".to_string()]);
        assert!(caps.local_inhibit);
        assert!(!caps.countdown);
        assert!(caps.holds_last_on_disconnect);
        assert!(caps.can_enforce_locally("door_open"));
        assert!(caps.can_fail_safe(), "local_inhibit alone fails safe");
    }
}
