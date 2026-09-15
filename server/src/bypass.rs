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
use css_lib::bypass::{classify_liveness, LivenessTransition};
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
