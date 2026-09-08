//! Membership dues ledger: entitlement, accrual, and the renewal cycle.
//!
//! Membership is driven by a **non-negative credit balance**. Payments (Stripe
//! recurring, Stripe one-shot, admin-logged cash) post credits; a period's dues
//! post a debit, but **only when the balance can cover them**, so the balance
//! never goes negative. Entitlement (holding an active, dues-covered membership)
//! drives the user's role between `member_role` and `lapsed_role` -- it never
//! touches `is_active`, so a lapsed member can still log in and pay.
//!
//! The load-bearing decisions are pure and unit-tested:
//! * [`plan_role_transition`] -- the role state machine, including the
//!   last-admin guard and the never-restore-elevated rule.
//! * [`advance_period`] -- calendar-aware anniversary advance, rolling a
//!   non-existent day (a 31st, a leap-day) to the first of the next month.
//! * [`dues_due`] -- the anniversary + grace boundary.
//!
//! Two mechanisms keep the ledger correct:
//! * the **webhook path** posts a credit as soon as Stripe reports a payment;
//! * the **renewal cycle** ([`MembershipService::run_cycle`], the daily ticker
//!   and the admin "reconcile now") is the guaranteed backbone: it re-credits
//!   any paid Stripe invoice missing from the ledger (so correctness never
//!   depends on webhook delivery), then deducts due periods or lapses members
//!   who cannot cover them.

use std::sync::Arc;

use bigdecimal::BigDecimal;
use chrono::{DateTime, Datelike, Duration, Months, Utc};
use serde::Serialize;
use std::str::FromStr;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::{ConfigManager, MembershipPeriod};
use crate::database::{DatabaseError, DatabaseManager};
use crate::doors::DoorService;
use crate::models::{
    AuditEventType, LedgerEntryType, NewAuditLog, NewMembershipLedgerEntry, NewMembershipSyncRun,
    UpdateUser, User, UserRole,
};
use crate::stripe::StripeClient;

/// The role change (if any) a membership event implies.
///
/// * `current` -- the user's role now.
/// * `entitled` -- true iff, after this event, the user holds an active,
///   dues-covered membership.
/// * `is_last_admin` -- true iff `current == Admin` and they are the only active
///   admin.
///
/// Returns the role to write, or `None` for no change. Rules, first match wins:
/// 1. entitled and below `member_role` -> grant `member_role`.
/// 2. not entitled, last admin -> no change (guard).
/// 3. not entitled, above `lapsed_role` -> revoke to `lapsed_role`.
/// 4. otherwise -> no change.
///
/// Grants only ever yield `member_role` (validated `<= Member`), so an elevated
/// role is never restored automatically: a Staff/Admin who lapsed to Newbie and
/// pays again returns as Member, not their old role.
pub fn plan_role_transition(
    current: &UserRole,
    entitled: bool,
    is_last_admin: bool,
    member_role: &UserRole,
    lapsed_role: &UserRole,
) -> Option<UserRole> {
    if entitled {
        if current.rank() < member_role.rank() {
            return Some(member_role.clone());
        }
        return None;
    }
    if *current == UserRole::Admin && is_last_admin {
        return None;
    }
    if current.rank() > lapsed_role.rank() {
        return Some(lapsed_role.clone());
    }
    None
}

/// Advance an anniversary by one dues period, calendar-aware.
///
/// Adding months clamps a too-large day to the target month's last day (chrono's
/// behavior). When that happens -- a 29th/30th/31st anniversary, or a leap-day --
/// we roll to the **first day of the following month** (the "weird day" rule):
/// the clamped date is always a month's last day, so one more day lands on the
/// first of the next. Days 1-28 exist in every month and are never rolled.
pub fn advance_period(date: DateTime<Utc>, period: MembershipPeriod) -> DateTime<Utc> {
    let months = match period {
        MembershipPeriod::Monthly => 1,
        MembershipPeriod::Quarterly => 3,
        MembershipPeriod::Yearly => 12,
    };
    let orig_day = date.day();
    let advanced = date.checked_add_months(Months::new(months)).unwrap_or(date);
    if advanced.day() != orig_day {
        advanced + Duration::days(1)
    } else {
        advanced
    }
}

/// Whether a period's dues are due for evaluation: the anniversary has passed by
/// at least `grace_days` (so a renewal payment has had time to land).
pub fn dues_due(next_due: DateTime<Utc>, now: DateTime<Utc>, grace_days: i64) -> bool {
    now >= next_due + Duration::days(grace_days)
}

/// The result of one renewal-cycle pass, for the run log and admin view.
#[derive(Debug, Default, Clone, Serialize)]
pub struct MembershipCycleOutcome {
    pub users_checked: usize,
    pub dues_charged: usize,
    pub lapsed: usize,
    /// Paid Stripe invoices re-credited by the poll backbone this pass.
    pub credited: usize,
    pub errors: usize,
    pub ok: bool,
    pub error: Option<String>,
}

/// The membership-billing service: ledger writes, entitlement, and the cycle.
#[derive(Clone)]
pub struct MembershipService {
    db: Arc<DatabaseManager>,
    config: Arc<ConfigManager>,
    door_service: Arc<DoorService>,
    /// `None` when Stripe is disabled (cash-only operation).
    stripe: Option<StripeClient>,
}

impl MembershipService {
    pub fn new(
        db: Arc<DatabaseManager>,
        config: Arc<ConfigManager>,
        door_service: Arc<DoorService>,
        stripe: Option<StripeClient>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            door_service,
            stripe,
        })
    }

    fn enabled(&self) -> bool {
        self.config.get_config().membership.enabled
    }

    /// Configured dues amount. Boot validation guarantees this parses to a
    /// positive decimal when the module is enabled; a parse failure here would
    /// only be reachable with the module disabled, where it is never consulted.
    fn due_amount(&self) -> BigDecimal {
        let raw = self.config.get_config().membership.due_amount;
        BigDecimal::from_str(raw.trim()).unwrap_or_else(|_| {
            warn!("membership.due_amount ({raw:?}) did not parse; treating as 0");
            BigDecimal::from(0)
        })
    }

    fn period(&self) -> MembershipPeriod {
        self.config.get_config().membership.due_period
    }

    fn grace_days(&self) -> i64 {
        self.config.get_config().membership.grace_days
    }

    fn currency(&self) -> String {
        self.config.get_config().membership.currency
    }

    fn roles(&self) -> (UserRole, UserRole) {
        let c = self.config.get_config();
        (c.membership.member_role, c.membership.lapsed_role)
    }

    /// Post a credit and, if it starts a membership, deduct the first period and
    /// grant the role. Idempotent on `reference`: a redelivered Stripe event
    /// posts nothing a second time (checked here and enforced by the ledger's
    /// partial unique index, which this also tolerates on a race). Returns
    /// whether a new entry was posted.
    pub fn record_credit(
        &self,
        user: &User,
        entry_type: LedgerEntryType,
        amount: BigDecimal,
        reference: Option<String>,
        description: Option<String>,
        actor: Option<Uuid>,
    ) -> Result<bool, DatabaseError> {
        if let Some(r) = reference.as_deref() {
            if self.db.ledger_entry_exists_for_reference(r)? {
                return Ok(false);
            }
        }
        let entry = NewMembershipLedgerEntry {
            user_id: user.id,
            entry_type,
            amount,
            currency: self.currency(),
            occurred_at: Utc::now(),
            description,
            external_reference: reference,
            created_by: actor,
        };
        match self.db.insert_ledger_entry(&entry) {
            Ok(_) => {}
            // Lost the race with a concurrent redelivery: the unique index
            // rejected the duplicate. That is the intended outcome, not an error.
            Err(DatabaseError::Diesel(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ))) => return Ok(false),
            Err(e) => return Err(e),
        }
        self.settle_after_credit(user.id)?;
        Ok(true)
    }

    /// After a credit, start a membership if the user is not enrolled and now has
    /// enough to cover a period. An already-enrolled member just keeps the credit
    /// toward their next renewal (no immediate deduction, no role change).
    fn settle_after_credit(&self, uid: Uuid) -> Result<(), DatabaseError> {
        let Some(user) = self.db.find_user_by_id(uid)? else {
            return Ok(());
        };
        if user.membership_next_due_at.is_some() {
            return Ok(()); // already enrolled; credit accumulates
        }
        let due = self.due_amount();
        if self.db.user_balance(uid)? >= due {
            // Start membership: consume the first period, anchor the anniversary
            // to now, grant the role.
            self.post_dues(uid, &due)?;
            let next = advance_period(Utc::now(), self.period());
            self.db.set_membership_next_due(uid, Some(next))?;
            self.apply_role(&user, true)?;
        }
        Ok(())
    }

    /// Post a dues debit of `-amount`.
    fn post_dues(&self, uid: Uuid, amount: &BigDecimal) -> Result<(), DatabaseError> {
        let entry = NewMembershipLedgerEntry {
            user_id: uid,
            entry_type: LedgerEntryType::DuesCharge,
            amount: -amount.clone(),
            currency: self.currency(),
            occurred_at: Utc::now(),
            description: Some("Membership dues".to_string()),
            external_reference: None,
            created_by: None,
        };
        self.db.insert_ledger_entry(&entry)?;
        Ok(())
    }

    /// Apply the role state machine for one user at a known entitlement.
    fn apply_role(&self, user: &User, entitled: bool) -> Result<(), DatabaseError> {
        let (member_role, lapsed_role) = self.roles();
        let is_last_admin = user.role == UserRole::Admin && self.db.count_active_admins()? <= 1;

        if !entitled && user.role == UserRole::Admin && is_last_admin {
            // The guard fired: the demotion is refused. Recorded so the owner
            // sees an admin who owes dues but was protected.
            self.audit(
                AuditEventType::MembershipLastAdminProtected,
                Some(user.id),
                serde_json::json!({ "role": user.role }),
            );
        }

        if let Some(new_role) = plan_role_transition(
            &user.role,
            entitled,
            is_last_admin,
            &member_role,
            &lapsed_role,
        ) {
            let update = UpdateUser {
                username: None,
                email: None,
                password_hash: None,
                full_name: None,
                is_active: None,
                role: Some(new_role.clone()),
                profile: None,
                updated_at: Some(Utc::now().naive_utc()),
                meta: None,
            };
            self.db.update_user(user.id, &update)?;
            // A role change moves the user in/out of role-based door allow-lists.
            self.door_service.republish_all();
            let event = if entitled {
                AuditEventType::MembershipGranted
            } else {
                AuditEventType::MembershipRevoked
            };
            self.audit(
                event,
                Some(user.id),
                serde_json::json!({ "from": user.role, "to": new_role }),
            );
        }
        Ok(())
    }

    /// Run one renewal cycle: the Stripe poll backbone, then the dues pass.
    /// Safe to call from the ticker or the admin "reconcile now" endpoint.
    pub async fn run_cycle(&self) -> MembershipCycleOutcome {
        let started_at = Utc::now();
        let mut outcome = MembershipCycleOutcome::default();
        if !self.enabled() {
            outcome.error = Some("membership module is disabled".to_string());
            self.record_run(started_at, &outcome);
            return outcome;
        }

        // 1. Backbone: re-credit any paid Stripe invoice missing from the ledger,
        //    so a dropped webhook cannot wrongly lapse a paying member. Runs
        //    before the dues pass so a caught-up credit prevents a spurious lapse.
        if let Some(stripe) = &self.stripe {
            match self.db.users_with_stripe_customer() {
                Ok(users) => {
                    for user in users {
                        let Some(customer) = user.stripe_customer_id.clone() else {
                            continue;
                        };
                        match stripe.list_paid_invoices(&customer).await {
                            Ok(invoices) => {
                                for inv in invoices {
                                    match self.record_credit(
                                        &user,
                                        LedgerEntryType::StripePayment,
                                        minor_units_to_decimal(inv.amount_paid),
                                        Some(inv.id.clone()),
                                        Some("Stripe invoice (reconciled)".to_string()),
                                        None,
                                    ) {
                                        Ok(true) => {
                                            outcome.credited += 1;
                                            info!(
                                                "membership: reconcile credited invoice {} for {}",
                                                inv.id, user.email
                                            );
                                        }
                                        Ok(false) => {}
                                        Err(e) => {
                                            outcome.errors += 1;
                                            warn!("membership: reconcile credit failed: {e}");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                outcome.errors += 1;
                                warn!(
                                    "membership: list_paid_invoices for {} failed: {e}",
                                    user.email
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    outcome.error = Some(format!("load stripe-linked users: {e}"));
                    self.record_run(started_at, &outcome);
                    return outcome;
                }
            }
        }

        // 2. Dues pass, over users enrolled after the poll (so a membership the
        //    poll just started is included).
        let enrolled = match self.db.enrolled_users() {
            Ok(v) => v,
            Err(e) => {
                outcome.error = Some(format!("load enrolled users: {e}"));
                self.record_run(started_at, &outcome);
                return outcome;
            }
        };
        for user in enrolled {
            outcome.users_checked += 1;
            if let Err(e) = self.settle_renewal(&user, &mut outcome) {
                outcome.errors += 1;
                warn!("membership: renewal for {} failed: {e}", user.email);
            }
        }

        outcome.ok = outcome.error.is_none();
        self.record_run(started_at, &outcome);
        outcome
    }

    /// Deduct every period now due for one enrolled user, or lapse them the
    /// moment the balance cannot cover the next period. The loop catches a member
    /// up across a gap (server was down) or spends a large prepaid balance; it is
    /// bounded because each deduction advances the anniversary into the future.
    fn settle_renewal(
        &self,
        user: &User,
        outcome: &mut MembershipCycleOutcome,
    ) -> Result<(), DatabaseError> {
        let due = self.due_amount();
        let grace = self.grace_days();
        let now = Utc::now();
        // Re-read the user so `next_due` reflects the poll's writes this pass.
        let Some(mut user) = self.db.find_user_by_id(user.id)? else {
            return Ok(());
        };

        let mut guard = 0;
        while let Some(next_due) = user.membership_next_due_at {
            guard += 1;
            if guard > 1200 {
                // ~100 years of monthly periods: a runaway, not a real gap.
                warn!("membership: renewal loop cap hit for {}", user.email);
                break;
            }
            if !dues_due(next_due, now, grace) {
                break;
            }
            if self.db.user_balance(user.id)? >= due {
                self.post_dues(user.id, &due)?;
                let next = advance_period(next_due, self.period());
                self.db.set_membership_next_due(user.id, Some(next))?;
                outcome.dues_charged += 1;
                self.apply_role(&user, true)?;
                user.membership_next_due_at = Some(next);
            } else {
                // Cannot cover: end the membership (no debt), downgrade.
                self.db.set_membership_next_due(user.id, None)?;
                self.apply_role(&user, false)?;
                outcome.lapsed += 1;
                user.membership_next_due_at = None;
            }
        }
        Ok(())
    }

    fn record_run(&self, started_at: DateTime<Utc>, outcome: &MembershipCycleOutcome) {
        let run = NewMembershipSyncRun {
            started_at,
            finished_at: Utc::now(),
            users_checked: outcome.users_checked as i32,
            dues_charged: outcome.dues_charged as i32,
            lapsed: outcome.lapsed as i32,
            errors: outcome.errors as i32,
            ok: outcome.ok,
            error: outcome.error.clone(),
        };
        if let Err(e) = self.db.record_membership_sync_run(&run) {
            warn!("membership: failed to record sync run: {e}");
        }
    }

    /// Best-effort audit write; a failure never fails the operation.
    fn audit(&self, event: AuditEventType, user_id: Option<Uuid>, data: serde_json::Value) {
        let log = NewAuditLog {
            event_type: event.as_str().to_string(),
            user_id,
            actor_id: None,
            event_data: data,
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&log) {
            error!("membership: failed to write audit {}: {e}", event.as_str());
        }
    }
}

/// Convert an integer count of minor currency units (Stripe amounts, e.g. cents)
/// to a `BigDecimal` with two-place scale, exactly (no float, no division).
///
/// Built from a formatted decimal literal so it depends only on `BigDecimal`'s
/// `FromStr`, not on an optional `num-bigint` re-export.
pub fn minor_units_to_decimal(minor: i64) -> BigDecimal {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    BigDecimal::from_str(&format!("{sign}{}.{:02}", abs / 100, abs % 100))
        .expect("a formatted decimal literal always parses")
}

/// Convert a decimal amount to integer minor units (e.g. cents), rounding to the
/// nearest unit. Used to price a one-shot Stripe checkout inline. Reads the
/// rounded integer back through its string, so it depends only on `BigDecimal`'s
/// `Mul`/`round`/`Display`, not on an optional `num-traits` re-export.
pub fn decimal_to_minor_units(amount: &BigDecimal) -> i64 {
    let scaled = (amount.clone() * BigDecimal::from(100)).round(0);
    scaled.to_string().parse::<i64>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    // --- plan_role_transition: the full worked-cases table -------------------

    fn plan(current: UserRole, entitled: bool, last_admin: bool) -> Option<UserRole> {
        plan_role_transition(
            &current,
            entitled,
            last_admin,
            &UserRole::Member,
            &UserRole::Newbie,
        )
    }

    #[test]
    fn entitled_newbie_is_granted_member() {
        assert_eq!(plan(UserRole::Newbie, true, false), Some(UserRole::Member));
    }

    #[test]
    fn entitled_member_and_above_are_left_alone() {
        assert_eq!(plan(UserRole::Member, true, false), None);
        assert_eq!(plan(UserRole::Staff, true, false), None);
        assert_eq!(plan(UserRole::Admin, true, false), None);
    }

    #[test]
    fn lapsed_member_staff_admin_are_downgraded() {
        assert_eq!(plan(UserRole::Member, false, false), Some(UserRole::Newbie));
        // Staff and a non-last Admin are downgraded on lapse too.
        assert_eq!(plan(UserRole::Staff, false, false), Some(UserRole::Newbie));
        assert_eq!(plan(UserRole::Admin, false, false), Some(UserRole::Newbie));
    }

    #[test]
    fn the_last_admin_is_never_downgraded() {
        assert_eq!(plan(UserRole::Admin, false, true), None);
    }

    #[test]
    fn lapsed_newbie_and_unknown_have_nothing_to_strip() {
        assert_eq!(plan(UserRole::Newbie, false, false), None);
        assert_eq!(plan(UserRole::Unknown, false, false), None);
    }

    #[test]
    fn entitled_unknown_is_granted_member() {
        assert_eq!(plan(UserRole::Unknown, true, false), Some(UserRole::Member));
    }

    #[test]
    fn elevated_roles_are_never_restored_by_a_grant() {
        // A Staff who lapsed to Newbie and pays again comes back as Member only,
        // whatever the configured member_role -- a grant never yields Staff.
        let granted = plan_role_transition(
            &UserRole::Newbie,
            true,
            false,
            &UserRole::Member,
            &UserRole::Newbie,
        );
        assert_eq!(granted, Some(UserRole::Member));
        assert_ne!(granted, Some(UserRole::Staff));
    }

    // --- advance_period: calendar rollover ----------------------------------

    #[test]
    fn monthly_keeps_a_safe_day() {
        assert_eq!(
            advance_period(dt(2026, 1, 15), MembershipPeriod::Monthly).date_naive(),
            dt(2026, 2, 15).date_naive()
        );
        // Day 28 exists in every month, so it is never rolled.
        assert_eq!(
            advance_period(dt(2026, 1, 28), MembershipPeriod::Monthly).date_naive(),
            dt(2026, 2, 28).date_naive()
        );
    }

    #[test]
    fn monthly_rolls_a_nonexistent_day_to_the_first_of_next_month() {
        // Jan 31 -> (Feb has no 31st) -> Mar 1.
        assert_eq!(
            advance_period(dt(2026, 1, 31), MembershipPeriod::Monthly).date_naive(),
            dt(2026, 3, 1).date_naive()
        );
        // Jan 30 -> Mar 1 for the same reason (Feb 2026 is not a leap year).
        assert_eq!(
            advance_period(dt(2026, 1, 30), MembershipPeriod::Monthly).date_naive(),
            dt(2026, 3, 1).date_naive()
        );
    }

    #[test]
    fn yearly_leap_day_rolls_to_march_first() {
        // Feb 29 2028 (leap) + 1 year -> Feb 2029 has no 29th -> Mar 1.
        assert_eq!(
            advance_period(dt(2028, 2, 29), MembershipPeriod::Yearly).date_naive(),
            dt(2029, 3, 1).date_naive()
        );
    }

    #[test]
    fn quarterly_advances_three_months() {
        assert_eq!(
            advance_period(dt(2026, 1, 15), MembershipPeriod::Quarterly).date_naive(),
            dt(2026, 4, 15).date_naive()
        );
    }

    // --- dues_due: the grace boundary ---------------------------------------

    #[test]
    fn dues_are_not_due_before_the_grace_elapses() {
        let anniversary = dt(2026, 2, 15);
        // Exactly at the anniversary with a 1-day grace: not yet due.
        assert!(!dues_due(anniversary, dt(2026, 2, 15), 1));
        // The next day: due.
        assert!(dues_due(anniversary, dt(2026, 2, 16), 1));
        // With zero grace, due exactly at the anniversary.
        assert!(dues_due(anniversary, dt(2026, 2, 15), 0));
    }

    // --- minor-units conversion ---------------------------------------------

    #[test]
    fn minor_units_convert_without_float_error() {
        assert_eq!(
            minor_units_to_decimal(1050),
            BigDecimal::from_str("10.50").unwrap()
        );
        assert_eq!(
            minor_units_to_decimal(0),
            BigDecimal::from_str("0.00").unwrap()
        );
        assert_eq!(
            minor_units_to_decimal(7),
            BigDecimal::from_str("0.07").unwrap()
        );
    }

    #[test]
    fn decimal_to_minor_units_rounds_to_the_nearest_unit() {
        assert_eq!(
            decimal_to_minor_units(&BigDecimal::from_str("10.50").unwrap()),
            1050
        );
        assert_eq!(
            decimal_to_minor_units(&BigDecimal::from_str("25").unwrap()),
            2500
        );
        assert_eq!(
            decimal_to_minor_units(&BigDecimal::from_str("0.07").unwrap()),
            7
        );
        // Round-trips with the inverse.
        assert_eq!(
            minor_units_to_decimal(decimal_to_minor_units(
                &BigDecimal::from_str("30.00").unwrap()
            )),
            BigDecimal::from_str("30.00").unwrap()
        );
    }
}
