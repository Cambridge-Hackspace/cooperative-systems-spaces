//! Bypass detection (#84): notice and record tool misuse.
//!
//! This is **detection, not prevention**. A determined person with wire cutters
//! defeats a smart plug and nothing here changes that. What this produces is the
//! record that survives to be shown to an insurer -- and, just as importantly,
//! the record that means nobody has to take anybody's word for what happened.
//!
//! The decision logic lives in [`css_lib::bypass`] so it can be tested without a
//! database; this module is the part that reads the world and writes rows.
//!
//! **Everything here logs and nothing here acts.** Draw on a circuit is not
//! proof that a particular tool ran, and a detection signal is not grounds to
//! cut power to a machine somebody may have their hands inside. Enforcement is
//! #83's coordinator, which decides from authorization and interlocks rather
//! than from suspicion.

use std::sync::Arc;

use chrono::Utc;
use css_lib::bypass::{
    classify_liveness, is_unauthorized_power, LivenessTransition, PowerEvidence,
};
use tracing::{error, info};

use crate::config::BypassConfig;
use crate::database::DatabaseManager;
use crate::models::{AuditEventType, NewAuditLog};

/// Sweeps module liveness and records the transitions.
pub struct BypassService {
    db: Arc<DatabaseManager>,
    config: BypassConfig,
}

impl BypassService {
    pub fn new(db: Arc<DatabaseManager>, config: BypassConfig) -> Self {
        Self { db, config }
    }

    /// Run the sweep forever. Mirrors the doors schedule ticker and the
    /// membership renewal cycle: a periodic pass that writes audit rows.
    pub async fn run(self) {
        let period = std::time::Duration::from_secs(self.config.liveness_sweep_secs.max(1));
        let mut ticker = tokio::time::interval(period);
        // The first tick completes immediately; skipping it avoids reporting
        // every module as silent in the instant before any of them has had a
        // chance to check in after a restart.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = self.sweep_once() {
                error!("Bypass liveness sweep failed: {}", e);
            }
            if let Err(e) = self.check_unauthorized_power() {
                error!("Bypass unauthorized-power check failed: {}", e);
            }
        }
    }

    /// One pass: compare every bound module's last report against the threshold,
    /// and write a row only where that differs from what was last recorded.
    ///
    /// Returns how many transitions were written, which is what the tests
    /// assert on.
    pub fn sweep_once(&self) -> Result<usize, crate::database::DatabaseError> {
        let modules = self.db.bound_modules_with_liveness()?;
        if modules.is_empty() {
            return Ok(0);
        }
        let recorded = self.db.latest_module_liveness_records()?;
        let now = Utc::now();
        let mut written = 0;

        for (module_id, name, role, device_id, last_seen) in modules {
            let seconds_since_seen = last_seen.map(|t| (now - t).num_seconds());
            let previously_silent = recorded
                .get(&module_id.to_string())
                .map(|e| e == AuditEventType::ToolModuleSilent.as_str())
                .unwrap_or(false);

            let transition = classify_liveness(
                seconds_since_seen,
                self.config.module_silence_secs,
                previously_silent,
            );
            let event = match transition {
                LivenessTransition::NoChange => continue,
                LivenessTransition::WentSilent => AuditEventType::ToolModuleSilent,
                LivenessTransition::Returned => AuditEventType::ToolModuleReturned,
            };

            // What this row does NOT say is why. Unplugged, crashed, wifi
            // dropped, broker unreachable, or pulled off the wall on purpose all
            // look identical from here, and a field claiming to know which would
            // be wrong about a quarter of the time. It records silence, and for
            // how long.
            let data = serde_json::json!({
                "module_id": module_id,
                "module_name": name,
                "role": role,
                "device_id": device_id,
                "seconds_since_seen": seconds_since_seen,
                "threshold_secs": self.config.module_silence_secs,
                "note": "silence only; the cause (power, network, or removal) is not distinguishable from here",
            });
            self.write(event, data);
            written += 1;
            info!(
                "Bypass: module {} ({}) {}",
                name,
                role,
                match transition {
                    LivenessTransition::WentSilent => "went silent",
                    _ => "is reporting again",
                }
            );
        }
        Ok(written)
    }

    /// Reconcile what the modules report against what the server authorized.
    ///
    /// The server's record of "this tool is allowed to be on" is `tools.status`:
    /// `tool_on` sets it InUse and `tool_off` sets it Idle, for every tool --
    /// unlike a usage session, which only exists for metered ones. A tool that
    /// is powered while the server never turned it on is the physical
    /// side-button case #80 asks about, and the gap the interlock tiers cannot
    /// reach.
    ///
    /// Two independent oracles, recorded separately because they are different
    /// claims and an insurer will ask which one fired.
    pub fn check_unauthorized_power(&self) -> Result<usize, crate::database::DatabaseError> {
        let readings = self.db.list_tool_power_state()?;
        if readings.is_empty() {
            return Ok(0);
        }
        let already = self.db.latest_unauthorized_power_reports()?;
        let now = Utc::now();
        let mut written = 0;

        for reading in readings {
            let Some(tool) = self.db.get_tool_by_id(reading.tool_id)? else {
                continue;
            };
            let authorized = matches!(tool.status, crate::models::ToolStatus::InUse);

            let drawing = {
                use bigdecimal::ToPrimitive;
                reading
                    .last_draw_amps
                    .as_ref()
                    .and_then(|d| d.to_f64())
                    .map(|a| a >= self.config.unauthorized_power_draw_amps)
                    .unwrap_or(false)
            };
            let evidence = PowerEvidence {
                relay_reported_on: reading.last_relay_on.unwrap_or(false),
                draw_observed: drawing,
            };
            let seconds_observed = reading
                .power_evidence_since
                .map(|t| (now - t).num_seconds())
                .unwrap_or(0);

            let finding = is_unauthorized_power(
                evidence,
                authorized,
                seconds_observed,
                self.config.unauthorized_power_debounce_secs,
            );
            // Reported already *for this episode*? The evidence clock restarts
            // whenever the tool stops being powered, so a report that predates
            // the current run belongs to an earlier incident and does not
            // suppress this one.
            let reported = match (
                already.get(&reading.tool_id.to_string()),
                reading.power_evidence_since,
            ) {
                (Some(reported_at), Some(since)) => *reported_at >= since,
                (Some(_), None) => true,
                (None, _) => false,
            };

            // Transition, like the liveness sweep: report the onset, not the
            // continued existence. A tool left energized overnight is one row.
            if !finding || reported {
                continue;
            }

            let data = serde_json::json!({
                "tool_id": reading.tool_id,
                "tool_name": tool.name,
                "evidence": {
                    "relay_reported_on": evidence.relay_reported_on,
                    "draw_observed": evidence.draw_observed,
                    "last_draw_amps": reading.last_draw_amps.as_ref().map(|d| d.to_string()),
                    "draw_threshold_amps": self.config.unauthorized_power_draw_amps,
                },
                "seconds_observed": seconds_observed,
                "tool_status": format!("{:?}", tool.status),
                "note": "the server never authorized this tool on; recorded, not acted on -- draw on a circuit is not proof this tool ran",
            });
            self.write(AuditEventType::UnauthorizedPowerDetected, data);
            written += 1;
            info!(
                "Bypass: tool {} powered without authorization (relay={}, draw={})",
                tool.name, evidence.relay_reported_on, evidence.draw_observed
            );
        }
        Ok(written)
    }

    /// System-triggered, so there is no human actor to record.
    fn write(&self, event: AuditEventType, data: serde_json::Value) {
        let log = NewAuditLog {
            event_type: event.as_str().to_string(),
            user_id: None,
            actor_id: None,
            event_data: data,
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&log) {
            error!("Failed to write bypass audit log {}: {}", event.as_str(), e);
        }
    }
}
