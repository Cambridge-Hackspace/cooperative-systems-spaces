//! Metered pay-per-use tool billing (Phase 2).
//!
//! Reuses the Phase-1 credit ledger: a tool session posts a single `tool_usage`
//! debit when it settles, and the gate reads *available* balance (ledger balance
//! minus open holds). The load-bearing arithmetic is pure and unit-tested:
//! [`is_metered`], [`max_session_cost`], [`session_charge`], [`metered_access_ok`].
//!
//! **Money is always the last gate.** The live `tool_on` path checks training
//! first and only calls [`ToolBillingService::open_session`] once training
//! passes, so a member never has a hold placed or a charge posted for a tool they
//! were not eligible to use (see `api::toolguard`).

use std::str::FromStr;
use std::sync::Arc;

use bigdecimal::BigDecimal;
use chrono::Utc;
use tracing::{error, warn};
use uuid::Uuid;

use crate::config::{ActuationMode, BillingMode, ConfigManager};
use crate::database::{DatabaseError, DatabaseManager};
use crate::models::{
    AuditEventType, LedgerEntryType, NewAuditLog, NewMembershipLedgerEntry, NewToolUsageSession,
    Tool, ToolUsageSession, User,
};

fn zero() -> BigDecimal {
    BigDecimal::from(0)
}

fn is_positive(v: &Option<BigDecimal>) -> bool {
    v.as_ref().is_some_and(|x| *x > zero())
}

/// Whether a tool is metered: it has a flat fee or a per-minute rate set.
pub fn is_metered(flat_fee: &Option<BigDecimal>, rate_per_min: &Option<BigDecimal>) -> bool {
    is_positive(flat_fee) || is_positive(rate_per_min)
}

/// The most a session can cost: `flat + rate × max_minutes`. This is the prepaid
/// hold amount and the ceiling on any settled charge.
pub fn max_session_cost(
    flat_fee: &Option<BigDecimal>,
    rate_per_min: &Option<BigDecimal>,
    max_minutes: i32,
) -> BigDecimal {
    let flat = flat_fee.clone().unwrap_or_else(zero);
    let rate = rate_per_min.clone().unwrap_or_else(zero);
    (flat + rate * BigDecimal::from(max_minutes)).round(2)
}

/// The charge for a settled session: `flat + rate × min(minutes_used,
/// max_minutes)`, where `minutes_used = effective_seconds / 60`. `effective_
/// seconds` is already the wall-clock-capped reported usage (the caller caps it);
/// this additionally caps at `max_minutes` and rounds to two places. The result
/// never exceeds [`max_session_cost`], which is why the prepaid hold covers it.
pub fn session_charge(
    flat_fee: &Option<BigDecimal>,
    rate_per_min: &Option<BigDecimal>,
    effective_seconds: &BigDecimal,
    max_minutes: i32,
) -> BigDecimal {
    let flat = flat_fee.clone().unwrap_or_else(zero);
    let rate = rate_per_min.clone().unwrap_or_else(zero);
    let max_secs = BigDecimal::from(max_minutes) * BigDecimal::from(60);
    let billable = if *effective_seconds > max_secs {
        max_secs
    } else if *effective_seconds < zero() {
        zero()
    } else {
        effective_seconds.clone()
    };
    let time_charge = rate * billable / BigDecimal::from(60);
    (flat + time_charge).round(2)
}

/// The gate a metered tool must pass: membership (when required) and enough
/// available balance. Pure so it reads identically at all three enforcement
/// points (live guard, edge allow-list, web self-check).
pub fn metered_access_ok(
    is_member: bool,
    require_membership: bool,
    available: &BigDecimal,
    required: &BigDecimal,
) -> bool {
    (!require_membership || is_member) && available >= required
}

/// A snapshot of the config the metered gate needs, so `database.rs` (which has
/// no `ConfigManager`) can compute the per-user allow-list filter. Built by
/// [`ToolBillingService::gate`].
pub struct MeteredGate {
    pub require_membership: bool,
    pub member_role_rank: u8,
    /// Prepaid gates on the max session cost; postpaid on `min_balance`.
    pub prepaid: bool,
    pub min_balance: BigDecimal,
    pub default_max_minutes: i32,
    /// True when `actuation_mode = OnlineSynchronous`: metered tools carry a
    /// `requires_online` flag so the edge asks the server before energizing.
    pub online_sync: bool,
}

impl MeteredGate {
    /// Whether the edge must gate this tool on a synchronous server call.
    pub fn requires_online(&self, tool: &Tool) -> bool {
        self.online_sync && is_metered(&tool.usage_flat_fee, &tool.usage_rate_per_min)
    }
}

impl MeteredGate {
    /// Available balance a member needs to start this tool.
    pub fn required_for(&self, tool: &Tool) -> BigDecimal {
        if self.prepaid {
            max_session_cost(
                &tool.usage_flat_fee,
                &tool.usage_rate_per_min,
                tool.usage_max_session_minutes
                    .unwrap_or(self.default_max_minutes),
            )
        } else {
            self.min_balance.clone()
        }
    }

    /// Whether a member with `available` balance may use `tool`. Free
    /// (non-metered) tools are never gated by this.
    pub fn authorizes(&self, tool: &Tool, available: &BigDecimal, is_member: bool) -> bool {
        if !is_metered(&tool.usage_flat_fee, &tool.usage_rate_per_min) {
            return true;
        }
        metered_access_ok(
            is_member,
            self.require_membership,
            available,
            &self.required_for(tool),
        )
    }
}

/// The outcome of trying to activate a metered tool.
pub enum ActivationOutcome {
    /// Authorized; the hold session was opened.
    Authorized(Box<ToolUsageSession>),
    /// Denied, with a member-facing reason.
    Denied(String),
}

/// Metered tool-billing service: gate + hold at activation, usage accumulation,
/// and settle. Reuses the membership ledger; re-broadcasting the edge allow-list
/// after a hold/settle is the caller's job (it needs `AppState`).
#[derive(Clone)]
pub struct ToolBillingService {
    db: Arc<DatabaseManager>,
    config: Arc<ConfigManager>,
}

impl ToolBillingService {
    pub fn new(db: Arc<DatabaseManager>, config: Arc<ConfigManager>) -> Arc<Self> {
        Arc::new(Self { db, config })
    }

    pub fn enabled(&self) -> bool {
        self.config.get_config().tool_billing.enabled
    }

    fn billing_mode(&self) -> BillingMode {
        self.config.get_config().tool_billing.billing_mode
    }

    fn require_membership(&self) -> bool {
        self.config.get_config().tool_billing.require_membership
    }

    fn currency(&self) -> String {
        self.config.get_config().tool_billing.currency
    }

    fn min_balance(&self) -> BigDecimal {
        let raw = self.config.get_config().tool_billing.min_balance;
        BigDecimal::from_str(raw.trim()).unwrap_or_else(|_| {
            warn!("tool_billing.min_balance ({raw:?}) did not parse; treating as 0");
            zero()
        })
    }

    /// Billable-minute cap for a tool (its own value, or the global default).
    pub fn max_minutes(&self, tool: &Tool) -> i32 {
        tool.usage_max_session_minutes.unwrap_or_else(|| {
            self.config
                .get_config()
                .tool_billing
                .default_max_session_minutes
        })
    }

    pub fn tool_is_metered(&self, tool: &Tool) -> bool {
        is_metered(&tool.usage_flat_fee, &tool.usage_rate_per_min)
    }

    /// A config snapshot for the per-user allow-list filter in `database.rs`.
    pub fn gate(&self) -> MeteredGate {
        let c = self.config.get_config();
        MeteredGate {
            require_membership: c.tool_billing.require_membership,
            member_role_rank: c.membership.member_role.rank(),
            prepaid: matches!(c.tool_billing.billing_mode, BillingMode::Prepaid),
            min_balance: self.min_balance(),
            default_max_minutes: c.tool_billing.default_max_session_minutes,
            online_sync: matches!(
                c.tool_billing.actuation_mode,
                ActuationMode::OnlineSynchronous
            ),
        }
    }

    fn is_member(&self, user: &User) -> bool {
        user.role.rank() >= self.config.get_config().membership.member_role.rank()
    }

    /// What available balance a member needs to start this tool: the max session
    /// cost (prepaid) or the configured floor (postpaid).
    fn required_available(&self, tool: &Tool) -> BigDecimal {
        match self.billing_mode() {
            BillingMode::Prepaid => max_session_cost(
                &tool.usage_flat_fee,
                &tool.usage_rate_per_min,
                self.max_minutes(tool),
            ),
            BillingMode::Postpaid => self.min_balance(),
        }
    }

    /// Read-only gate for the allow-list and the web self-check: may this member
    /// currently start this metered tool? No hold is placed.
    pub fn metered_authorized(&self, user: &User, tool: &Tool) -> Result<bool, DatabaseError> {
        let available = self.db.available_balance(user.id)?;
        Ok(metered_access_ok(
            self.is_member(user),
            self.require_membership(),
            &available,
            &self.required_available(tool),
        ))
    }

    /// Live activation: run the money gate (training is the caller's earlier
    /// check) and, on pass, open the hold session. Returns a denial reason
    /// otherwise. Never places a hold when the gate fails.
    pub fn open_session(
        &self,
        user: &User,
        tool: &Tool,
    ) -> Result<ActivationOutcome, DatabaseError> {
        // A tool is InUse by one member at a time; a lingering open session means
        // the previous use never settled -- refuse rather than stack holds.
        if self.db.open_tool_session_for_tool(tool.id)?.is_some() {
            return Ok(ActivationOutcome::Denied(
                "Tool already has an open session".to_string(),
            ));
        }
        let available = self.db.available_balance(user.id)?;
        let required = self.required_available(tool);
        if self.require_membership() && !self.is_member(user) {
            return Ok(ActivationOutcome::Denied("Membership required".to_string()));
        }
        if available < required {
            return Ok(ActivationOutcome::Denied(format!(
                "Insufficient balance: {} available, {} required",
                available.with_scale(2),
                required.with_scale(2)
            )));
        }
        let hold = match self.billing_mode() {
            BillingMode::Prepaid => required.clone(),
            BillingMode::Postpaid => zero(),
        };
        let session = self.db.insert_tool_session(&NewToolUsageSession {
            tool_id: tool.id,
            user_id: user.id,
            started_at: Utc::now(),
            hold_amount: hold,
        })?;
        Ok(ActivationOutcome::Authorized(Box::new(session)))
    }

    /// Accumulate a validated device usage report onto the tool's open session.
    /// Returns `false` for a negative report or an orphan (no open session) so
    /// the caller can reject it. The wall-clock cap is applied at settle.
    pub fn record_usage(&self, tool_id: Uuid, seconds: f32) -> Result<bool, DatabaseError> {
        if !(seconds.is_finite() && seconds >= 0.0) {
            return Ok(false);
        }
        let Some(session) = self.db.open_tool_session_for_tool(tool_id)? else {
            return Ok(false); // orphan report: no open session for this tool
        };
        let dec = BigDecimal::from_str(&format!("{seconds:.3}")).unwrap_or_else(|_| zero());
        self.db.add_reported_seconds(session.id, dec)?;
        Ok(true)
    }

    /// Settle the tool's open session: compute the charge (reported usage capped
    /// at wall-clock and at max minutes), post one `tool_usage` ledger debit
    /// keyed on the session id (idempotent -- a replay hits the unique reference
    /// or the already-closed session), and close the session. Returns the charge
    /// if this call settled it.
    pub fn settle_open_session_for_tool(
        &self,
        tool: &Tool,
        status_on_close: &str,
    ) -> Result<Option<BigDecimal>, DatabaseError> {
        let Some(session) = self.db.open_tool_session_for_tool(tool.id)? else {
            return Ok(None);
        };
        let reported = session.reported_seconds.clone().unwrap_or_else(zero);
        // Wall-clock cap: cutting time can never exceed the time the tool was on.
        let elapsed = (Utc::now() - session.started_at).num_seconds().max(0);
        let elapsed_dec = BigDecimal::from(elapsed);
        let effective = if reported > elapsed_dec {
            elapsed_dec
        } else {
            reported.clone()
        };
        let charge = session_charge(
            &tool.usage_flat_fee,
            &tool.usage_rate_per_min,
            &effective,
            self.max_minutes(tool),
        );
        let reference = session.id.to_string();

        // Idempotency: the ledger's partial unique index on external_reference
        // means the debit posts at most once for this session.
        let posted = if self.db.ledger_entry_exists_for_reference(&reference)? {
            None
        } else {
            let entry = NewMembershipLedgerEntry {
                user_id: session.user_id,
                entry_type: LedgerEntryType::ToolUsage,
                amount: -charge.clone(),
                currency: self.currency(),
                occurred_at: Utc::now(),
                description: Some(format!("Tool usage: {}", tool.name)),
                external_reference: Some(reference),
                created_by: None,
            };
            match self.db.insert_ledger_entry(&entry) {
                Ok(e) => Some(e),
                // Lost the race with a concurrent settle: the debit is already in.
                Err(DatabaseError::Diesel(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                ))) => None,
                Err(e) => return Err(e),
            }
        };

        let closed = self.db.settle_tool_session(
            session.id,
            session.reported_seconds.clone(),
            charge.clone(),
            posted.as_ref().map(|e| e.id),
            status_on_close,
        )?;

        if closed && posted.is_some() {
            let event = if status_on_close == "abandoned" {
                AuditEventType::ToolSessionAbandoned
            } else {
                AuditEventType::ToolUsageCharged
            };
            self.audit(
                event,
                session.user_id,
                serde_json::json!({
                    "tool_id": tool.id,
                    "session_id": session.id,
                    "charge": charge.with_scale(2).to_string(),
                    "reported_seconds": effective.with_scale(0).to_string(),
                }),
            );
            return Ok(Some(charge));
        }
        Ok(None)
    }

    /// Settle sessions left open past the cap (never stopped, or an edge reboot),
    /// charging from reported usage. Returns how many it closed.
    pub fn sweep_abandoned(&self) -> Result<usize, DatabaseError> {
        if !self.enabled() {
            return Ok(0);
        }
        let default_max = self
            .config
            .get_config()
            .tool_billing
            .default_max_session_minutes
            .max(1) as i64;
        // A generous cutoff: the global default max plus an hour of grace.
        let cutoff = Utc::now() - chrono::Duration::minutes(default_max + 60);
        let stale = self.db.open_tool_sessions_older_than(cutoff)?;
        let mut closed = 0;
        for session in stale {
            let Some(tool) = self.db.get_tool_by_id(session.tool_id)? else {
                continue;
            };
            match self.settle_open_session_for_tool(&tool, "abandoned") {
                Ok(Some(_)) => closed += 1,
                Ok(None) => {}
                Err(e) => warn!("tool-billing sweep: settle failed: {e}"),
            }
        }
        Ok(closed)
    }

    fn audit(&self, event: AuditEventType, user_id: Uuid, data: serde_json::Value) {
        let log = NewAuditLog {
            event_type: event.as_str().to_string(),
            user_id: Some(user_id),
            actor_id: None,
            event_data: data,
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&log) {
            error!(
                "tool-billing: failed to write audit {}: {e}",
                event.as_str()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bd(s: &str) -> BigDecimal {
        BigDecimal::from_str(s).unwrap()
    }

    #[test]
    fn metered_iff_a_fee_or_rate_is_positive() {
        assert!(!is_metered(&None, &None));
        assert!(!is_metered(&Some(bd("0")), &Some(bd("0"))));
        assert!(is_metered(&Some(bd("0.50")), &None));
        assert!(is_metered(&None, &Some(bd("0.20"))));
    }

    #[test]
    fn max_session_cost_is_flat_plus_rate_times_max() {
        // 0.50 flat + 0.20/min * 120 min = 24.50
        assert_eq!(
            max_session_cost(&Some(bd("0.50")), &Some(bd("0.20")), 120),
            bd("24.50")
        );
        // rate-only
        assert_eq!(max_session_cost(&None, &Some(bd("0.20")), 60), bd("12.00"));
        // flat-only
        assert_eq!(max_session_cost(&Some(bd("5")), &None, 120), bd("5.00"));
    }

    #[test]
    fn session_charge_is_flat_plus_time() {
        // 0.50 flat + 0.20/min * 5 min (300s) = 1.50
        assert_eq!(
            session_charge(&Some(bd("0.50")), &Some(bd("0.20")), &bd("300"), 120),
            bd("1.50")
        );
    }

    #[test]
    fn session_charge_caps_time_at_max_minutes() {
        // Reported 1,000,000s but max is 2 min: 0.20 * 2 = 0.40 (+ 0 flat).
        assert_eq!(
            session_charge(&None, &Some(bd("0.20")), &bd("1000000"), 2),
            bd("0.40")
        );
    }

    #[test]
    fn session_charge_never_exceeds_max_session_cost() {
        let flat = Some(bd("0.50"));
        let rate = Some(bd("0.20"));
        let max = max_session_cost(&flat, &rate, 120);
        let huge = session_charge(&flat, &rate, &bd("99999999"), 120);
        assert_eq!(huge, max, "a capped charge must equal the hold ceiling");
    }

    #[test]
    fn session_charge_treats_negative_seconds_as_zero() {
        assert_eq!(
            session_charge(&Some(bd("0.50")), &Some(bd("0.20")), &bd("-100"), 120),
            bd("0.50")
        );
    }

    #[test]
    fn gate_requires_membership_and_funds() {
        let ten = bd("10");
        let five = bd("5");
        // member, enough funds -> ok
        assert!(metered_access_ok(true, true, &ten, &five));
        // not a member, membership required -> denied even with funds
        assert!(!metered_access_ok(false, true, &ten, &five));
        // not a member, membership NOT required -> ok
        assert!(metered_access_ok(false, false, &ten, &five));
        // member, insufficient funds -> denied
        assert!(!metered_access_ok(true, true, &five, &ten));
        // exactly enough -> ok (>=)
        assert!(metered_access_ok(true, true, &ten, &ten));
    }
}
