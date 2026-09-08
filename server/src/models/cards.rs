use crate::models::User;
use crate::schema::{sql_types, user_cards};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

/// Lifecycle state of a member access card.
///
/// - `Active` — opens the member's tools/doors.
/// - `Disabled` — a known card deliberately turned off (lost, suspected
///   compromised, member on hold). Denies access, but the code stays bound to
///   the member so a presentation is attributable.
/// - `Released` — returned to the pool; the code may later be reissued to a
///   different member. Denies access under the old owner.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    diesel::AsExpression,
    diesel::FromSqlRow,
)]
#[diesel(sql_type = sql_types::CardStatus)]
#[serde(rename_all = "snake_case")]
pub enum CardStatus {
    Active,
    Disabled,
    Released,
}

impl CardStatus {
    /// Whether a card in this state may open a tool/door.
    pub fn grants_access(&self) -> bool {
        matches!(self, CardStatus::Active)
    }

    /// Whether presenting a card in this state is a revoked-credential event
    /// (known card, deliberately not active) worth a high-signal audit entry.
    pub fn is_revoked(&self) -> bool {
        matches!(self, CardStatus::Disabled | CardStatus::Released)
    }
}

impl diesel::serialize::ToSql<sql_types::CardStatus, diesel::pg::Pg> for CardStatus {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        match self {
            CardStatus::Active => out.write_all(b"active")?,
            CardStatus::Disabled => out.write_all(b"disabled")?,
            CardStatus::Released => out.write_all(b"released")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::CardStatus, diesel::pg::Pg> for CardStatus {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"active" => Ok(CardStatus::Active),
            b"disabled" => Ok(CardStatus::Disabled),
            b"released" => Ok(CardStatus::Released),
            _ => Err("Unrecognized card_status variant".into()),
        }
    }
}

/// A member access card (RFID/fob/sticker/phone tag).
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = user_cards)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserCard {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code: String,
    pub status: CardStatus,
    pub last_used_at: Option<DateTime<Utc>>,
    pub issued_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
    pub disabled_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert shape for a new card. `status` defaults to `active` at the DB level
/// when omitted.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_cards)]
pub struct NewUserCard {
    pub user_id: Uuid,
    pub code: String,
    pub status: Option<CardStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_grants_access() {
        assert!(CardStatus::Active.grants_access());
        assert!(!CardStatus::Disabled.grants_access());
        assert!(!CardStatus::Released.grants_access());
    }

    #[test]
    fn disabled_and_released_are_revoked_but_active_is_not() {
        // grants_access and is_revoked must partition the states: a card is
        // exactly one of "opens the tool" or "worth a fraud event", never both
        // and never neither. This is what lets the resolver map each state to a
        // single arm.
        for status in [
            CardStatus::Active,
            CardStatus::Disabled,
            CardStatus::Released,
        ] {
            assert_ne!(
                status.grants_access(),
                status.is_revoked(),
                "{status:?} must be exactly one of granting or revoked"
            );
        }
        assert!(CardStatus::Disabled.is_revoked());
        assert!(CardStatus::Released.is_revoked());
        assert!(!CardStatus::Active.is_revoked());
    }

    #[test]
    fn serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&CardStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&CardStatus::Disabled).unwrap(),
            "\"disabled\""
        );
        assert_eq!(
            serde_json::to_string(&CardStatus::Released).unwrap(),
            "\"released\""
        );
    }
}

/// Outcome of resolving a scanned card code to a member.
///
/// The resolver deliberately does **not** collapse a revoked card into "not
/// found": a known-but-revoked card must be distinguishable from a code we've
/// never seen so the auth path can emit a fraud-signal audit event for the
/// former while keeping the latter a quiet denial.
pub enum CardResolution {
    /// An active card (or a legacy `profile_field` value, which has no stored
    /// status and is treated as active). `card` is `None` for the legacy path.
    Active { user: User, card: Option<UserCard> },
    /// A known card that is disabled or released. Access is denied and the
    /// presentation is attributable to `user`.
    Revoked { user: User, card: UserCard },
    /// No card row and no legacy profile-field match — an unknown code.
    Unknown,
}
