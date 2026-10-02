//! The alert heartbeat (#87).
//!
//! Once per `[alerts].heartbeat_interval_secs` (a week by default) the server
//! emits an `alert_heartbeat` audit event summarising the interval: alerts
//! raised, alerts still unacknowledged, webhook deliveries that failed. The
//! event is classified Notice, so it appears in the alert feed and reaches
//! every webhook whose subscription covers it -- the notification feed needs
//! no special case for it. What the operator watches for is its ABSENCE.
//!
//! The schedule is persisted, not counted from boot: an hourly ticker asks
//! whether the latest recorded run is older than the interval. A process that
//! restarts every few days still heartbeats on time, and a heartbeat is never
//! emitted twice for one interval across a restart.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use tracing::{info, warn};

use crate::database::DatabaseManager;
use crate::models::{AuditEventType, Category, NewAlertHeartbeatRun, Severity};
use crate::profile::AuditLogger;
use crate::shutdown::Shutdown;

/// How often the ticker checks whether a heartbeat is due. Much shorter than
/// the interval itself so a due heartbeat is at most this late.
const CHECK_EVERY_SECS: u64 = 3600;

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatOutcome {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    /// Audit events at Notice or above since the previous heartbeat (or ever,
    /// for the first), excluding the alerts category itself so a heartbeat
    /// never counts the last heartbeat as an alert.
    pub alerts_raised: i64,
    /// Alerts (Notice and above) with no acknowledgement right now.
    pub unacknowledged: i64,
    /// Webhook delivery attempts that failed since the previous heartbeat.
    pub deliveries_failed: i64,
    pub ok: bool,
    pub error: Option<String>,
}

pub struct HeartbeatService {
    db: Arc<DatabaseManager>,
    audit: AuditLogger,
    interval_secs: u64,
}

impl HeartbeatService {
    pub fn new(db: Arc<DatabaseManager>, audit: AuditLogger, interval_secs: u64) -> Self {
        Self {
            db,
            audit,
            interval_secs,
        }
    }

    /// Whether a heartbeat is due: no run recorded, or the latest started
    /// more than the interval ago. An interval of 0 disables the heartbeat.
    pub fn due(&self, now: DateTime<Utc>) -> Result<bool, crate::database::DatabaseError> {
        if self.interval_secs == 0 {
            return Ok(false);
        }
        let last = self.db.latest_alert_heartbeat_run()?;
        Ok(match last {
            None => true,
            Some(run) => now - run.started_at >= Duration::seconds(self.interval_secs as i64),
        })
    }

    /// The event types the heartbeat counts as alerts: Notice and above,
    /// excluding the alerts category itself.
    fn counted_types() -> Vec<String> {
        AuditEventType::all()
            .iter()
            .filter(|t| t.severity() >= Severity::Notice && t.category() != Category::Alerts)
            .map(|t| t.as_str().to_string())
            .collect()
    }

    /// Emit one heartbeat now, whether or not it is due (the admin endpoint
    /// and the e2e stage use this; the ticker checks `due` first).
    pub async fn run(&self) -> HeartbeatOutcome {
        let started_at = Utc::now();
        let since = match self.db.latest_alert_heartbeat_run() {
            Ok(Some(run)) => Some(run.started_at),
            Ok(None) => None,
            Err(e) => {
                return self
                    .finish(started_at, 0, 0, 0, Some(format!("load last run: {e}")))
                    .await
            }
        };
        let types = Self::counted_types();
        let alerts_raised = match self.db.count_audit_rows_since(&types, since) {
            Ok(n) => n,
            Err(e) => {
                return self
                    .finish(started_at, 0, 0, 0, Some(format!("count alerts: {e}")))
                    .await
            }
        };
        let unacknowledged = match self.db.unacknowledged_alert_counts(&types) {
            Ok(rows) => rows.into_iter().map(|(_, n)| n).sum(),
            Err(e) => {
                return self
                    .finish(
                        started_at,
                        alerts_raised,
                        0,
                        0,
                        Some(format!("count unacknowledged: {e}")),
                    )
                    .await
            }
        };
        let deliveries_failed = match self.db.count_failed_webhook_deliveries_since(since) {
            Ok(n) => n,
            Err(e) => {
                return self
                    .finish(
                        started_at,
                        alerts_raised,
                        unacknowledged,
                        0,
                        Some(format!("count failed deliveries: {e}")),
                    )
                    .await
            }
        };
        self.finish(
            started_at,
            alerts_raised,
            unacknowledged,
            deliveries_failed,
            None,
        )
        .await
    }

    async fn finish(
        &self,
        started_at: DateTime<Utc>,
        alerts_raised: i64,
        unacknowledged: i64,
        deliveries_failed: i64,
        error: Option<String>,
    ) -> HeartbeatOutcome {
        let ok = error.is_none();
        // The event first: the heartbeat IS the event. A run that could not
        // count still beats -- a heartbeat saying "counting failed" is a
        // signal; silence is not.
        if let Err(e) = self
            .audit
            .log_event(
                AuditEventType::AlertHeartbeat,
                None,
                None,
                serde_json::json!({
                    "since": self.db.latest_alert_heartbeat_run().ok().flatten().map(|r| r.started_at),
                    "alerts_raised": alerts_raised,
                    "unacknowledged": unacknowledged,
                    "deliveries_failed": deliveries_failed,
                    "ok": ok,
                    // `detail`, not `error`: an audit payload is not a response
                    // body, and an object with an `error` key reads as the error
                    // envelope (checks/tests/one_error_envelope.rs).
                    "detail": error.as_deref(),
                    "interval_secs": self.interval_secs,
                }),
                None,
                None,
            )
            .await
        {
            warn!("alert heartbeat: could not write the heartbeat event: {e}");
        }
        let finished_at = Utc::now();
        let run = NewAlertHeartbeatRun {
            started_at,
            finished_at,
            alerts_raised: alerts_raised as i32,
            unacknowledged: unacknowledged as i32,
            deliveries_failed: deliveries_failed as i32,
            ok,
            error: error.clone(),
        };
        if let Err(e) = self.db.record_alert_heartbeat_run(&run) {
            // Logged, not propagated: the next check would simply beat again.
            warn!("alert heartbeat: could not record the run: {e}");
        }
        HeartbeatOutcome {
            started_at,
            finished_at,
            alerts_raised,
            unacknowledged,
            deliveries_failed,
            ok,
            error,
        }
    }

    /// The ticker: hourly, beat if due. Returns when `shutdown` fires.
    pub async fn ticker(self: Arc<Self>, shutdown: Shutdown) {
        if self.interval_secs == 0 {
            info!("Alert heartbeat disabled (heartbeat_interval_secs = 0)");
            return;
        }
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(CHECK_EVERY_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = ticker.tick() => {}
            }
            match self.due(Utc::now()) {
                Ok(true) => {
                    let outcome = self.run().await;
                    info!(
                        "Alert heartbeat: {} alerts raised, {} unacknowledged, {} deliveries failed (ok={})",
                        outcome.alerts_raised,
                        outcome.unacknowledged,
                        outcome.deliveries_failed,
                        outcome.ok
                    );
                }
                Ok(false) => {}
                Err(e) => warn!("alert heartbeat: could not decide whether one is due: {e}"),
            }
        }
    }
}
