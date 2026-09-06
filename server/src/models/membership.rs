//! Data models for the membership dues ledger.
//!
//! The ledger is a per-member, non-negative credit account. Credits are
//! payments (Stripe recurring, Stripe one-shot, admin-logged cash); debits are
//! periodic dues and the rare refund/adjustment. Balance is `SUM(amount)` -- see
//! `DatabaseManager::user_balance` -- so these rows are the single source of
//! truth for entitlement; there is no cached balance column to drift.

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Kind of ledger entry, matching the Postgres `ledger_entry_type` enum.
///
/// Open to extension: Phase 2 (metered pay-per-use tool billing) will add a
/// `ToolUsage` variant + migration value.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow,
)]
#[diesel(sql_type = crate::schema::sql_types::LedgerEntryType)]
#[serde(rename_all = "snake_case")]
pub enum LedgerEntryType {
    /// A successful Stripe payment (recurring invoice or one-shot). Credit.
    StripePayment,
    /// An admin-logged cash (or otherwise off-Stripe) payment. Credit.
    CashPayment,
    /// A period's membership dues. Debit; posted only when the balance covers it.
    DuesCharge,
    /// A Stripe refund returned to the member. Debit.
    StripeRefund,
    /// A manual admin correction. Sign depends on the amount.
    Adjustment,
    /// A metered tool-use charge (Phase 2). Debit; posted when a tool session
    /// settles.
    ToolUsage,
}

impl LedgerEntryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LedgerEntryType::StripePayment => "stripe_payment",
            LedgerEntryType::CashPayment => "cash_payment",
            LedgerEntryType::DuesCharge => "dues_charge",
            LedgerEntryType::StripeRefund => "stripe_refund",
            LedgerEntryType::Adjustment => "adjustment",
            LedgerEntryType::ToolUsage => "tool_usage",
        }
    }
}

impl diesel::serialize::ToSql<crate::schema::sql_types::LedgerEntryType, diesel::pg::Pg>
    for LedgerEntryType
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        use std::io::Write;
        out.write_all(self.as_str().as_bytes())?;
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<crate::schema::sql_types::LedgerEntryType, diesel::pg::Pg>
    for LedgerEntryType
{
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"stripe_payment" => Ok(LedgerEntryType::StripePayment),
            b"cash_payment" => Ok(LedgerEntryType::CashPayment),
            b"dues_charge" => Ok(LedgerEntryType::DuesCharge),
            b"stripe_refund" => Ok(LedgerEntryType::StripeRefund),
            b"adjustment" => Ok(LedgerEntryType::Adjustment),
            b"tool_usage" => Ok(LedgerEntryType::ToolUsage),
            _ => Err("Unrecognized ledger_entry_type variant".into()),
        }
    }
}

/// One posted ledger entry.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::membership_ledger)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MembershipLedgerEntry {
    pub id: Uuid,
    pub user_id: Uuid,
    pub entry_type: LedgerEntryType,
    /// Signed: credits positive, debits (dues/refunds) negative.
    pub amount: BigDecimal,
    pub currency: String,
    pub occurred_at: DateTime<Utc>,
    pub description: Option<String>,
    /// Stripe invoice/charge id for idempotency; NULL for manual/dues entries.
    /// Never card data.
    pub external_reference: Option<String>,
    /// The admin who posted a manual entry; NULL for system/Stripe entries.
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A ledger entry to insert.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::membership_ledger)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewMembershipLedgerEntry {
    pub user_id: Uuid,
    pub entry_type: LedgerEntryType,
    pub amount: BigDecimal,
    pub currency: String,
    pub occurred_at: DateTime<Utc>,
    pub description: Option<String>,
    pub external_reference: Option<String>,
    pub created_by: Option<Uuid>,
}

/// One recorded membership renewal-cycle pass, for the admin status view.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::membership_sync_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MembershipSyncRun {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub users_checked: i32,
    pub dues_charged: i32,
    pub lapsed: i32,
    pub errors: i32,
    pub ok: bool,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A renewal-cycle pass to record.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::membership_sync_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewMembershipSyncRun {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub users_checked: i32,
    pub dues_charged: i32,
    pub lapsed: i32,
    pub errors: i32,
    pub ok: bool,
    pub error: Option<String>,
}
