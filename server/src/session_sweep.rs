//! The session sweep (#173): nothing else returns a tool from `in_use` to `idle`.
//!
//! Firmware is expected to release the tool it opened -- `tool-off` on every
//! stop, including the ugly ones -- and FIRMWARE.md says so. This is the net
//! under that instruction, not the mechanism. A tool whose session has run past
//! its own cap plus a grace is returned to `idle`, its open billing session (if
//! any) settled as abandoned, and a Warning-severity audit event raised: a tool
//! that needed the sweep is a device that did not do its job, and it will go on
//! not doing it until someone looks.
//!
//! Independent of billing on purpose. With `tool_billing.enabled = false` -- the
//! default -- an unmetered tool has no session row for a billing sweep to find,
//! so a billing-gated sweep would leave exactly the common case stranded. The
//! only other automatic recovery, `boot-reset`, is site-wide and unsafe to
//! automate.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::ConfigManager;
use crate::database::{DatabaseError, DatabaseManager};
use crate::models::{AuditEventType, NewAuditLog, NewToolEvent, Tool, ToolStatus};
use crate::tool_billing::{abandoned_cutoff, session_cap_minutes, ToolBillingService};

pub struct SessionSweep {
    db: Arc<DatabaseManager>,
    config: Arc<ConfigManager>,
    /// Present only when billing is enabled. The release does not depend on it.
    billing: Option<Arc<ToolBillingService>>,
}

impl SessionSweep {
    pub fn new(
        db: Arc<DatabaseManager>,
        config: Arc<ConfigManager>,
        billing: Option<Arc<ToolBillingService>>,
    ) -> Self {
        Self {
            db,
            config,
            billing,
        }
    }

    /// One pass. Returns how many tools were released.
    pub fn sweep_once(&self) -> Result<usize, DatabaseError> {
        let cfg = self.config.get_config();
        let grace = cfg.tools.session_grace_minutes;
        let default_cap = cfg.tool_billing.default_max_session_minutes;
        let now = Utc::now();

        let mut released = 0;
        for tool in self.db.get_inuse_tools()? {
            let activation = self.db.latest_activation(tool.id)?;
            // A tool put `in_use` by hand has no activation event; its last
            // change is the honest start of the clock.
            let since = activation
                .as_ref()
                .map_or(tool.updated_at, |e| e.created_at);
            let cap = session_cap_minutes(tool.usage_max_session_minutes, default_cap);
            if since >= abandoned_cutoff(cap, grace, now) {
                continue;
            }
            self.release(&tool, since, cap, grace, activation.and_then(|e| e.user_id))?;
            released += 1;
        }

        // Billing sessions whose tool is somehow no longer `in_use`: settled on
        // the billing side alone, by the per-tool rule #171 gave it.
        if let Some(billing) = &self.billing {
            match billing.sweep_abandoned() {
                Ok(0) => {}
                Ok(n) => info!("session sweep: settled {n} orphaned billing session(s)"),
                Err(e) => warn!("session sweep: billing settle failed: {e}"),
            }
        }
        Ok(released)
    }

    fn release(
        &self,
        tool: &Tool,
        since: DateTime<Utc>,
        cap: i32,
        grace: i64,
        member: Option<Uuid>,
    ) -> Result<(), DatabaseError> {
        // Money first, status second, and the status change does not wait on
        // the money: a settle that fails is logged, and the tool is still
        // returned to the next member.
        let settled = match &self.billing {
            Some(billing) => match billing.settle_open_session_for_tool(tool, "abandoned") {
                Ok(settled) => settled.is_some(),
                Err(e) => {
                    warn!(
                        "session sweep: settling {}'s session failed ({e}); releasing anyway",
                        tool.name
                    );
                    false
                }
            },
            None => false,
        };

        self.db.update_tool_status(tool.id, &ToolStatus::Idle)?;
        self.db.create_tool_event(&NewToolEvent {
            tool_id: tool.id,
            event_type: "deactivated".to_string(),
            old_status: Some(tool.status.clone()),
            new_status: Some(ToolStatus::Idle),
            user_id: member,
            actor_id: None,
            notes: Some(format!(
                "Released by the session sweep: in use since {since}, cap {cap} min + \
                 {grace} min grace, and no tool-off arrived"
            )),
            scan_data: Some(serde_json::json!({
                "reason": "session-sweep",
                "in_use_since": since,
                "cap_minutes": cap,
                "grace_minutes": grace,
                "billing_session_settled": settled,
            })),
        })?;

        warn!(
            "session sweep: released {} (in use since {since}, cap {cap} min + {grace} min \
             grace); the device that opened this session never closed it",
            tool.name
        );
        let audit = NewAuditLog {
            event_type: AuditEventType::ToolReleasedBySweep.as_str().to_string(),
            user_id: member,
            actor_id: None,
            event_data: serde_json::json!({
                "tool_id": tool.id,
                "tool_name": tool.name,
                "in_use_since": since,
                "cap_minutes": cap,
                "grace_minutes": grace,
                "billing_session_settled": settled,
            }),
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&audit) {
            error!("session sweep: failed to write audit tool_released_by_sweep: {e}");
        }
        Ok(())
    }
}
