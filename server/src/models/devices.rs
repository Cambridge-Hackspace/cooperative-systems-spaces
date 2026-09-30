use crate::schema::{space_device_auth, space_device_auth_requests, space_devices, sql_types};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

/// The roles a device can declare in its `capabilities` (#101, replacing the old
/// single-valued `SpaceDeviceKind`).
///
/// `reader` / `power` / `sensor` are a tool's access-chain roles and `edge` is a
/// door's coordinator -- together the bindable `binding_role` set, which this
/// vocabulary must contain, because a device cannot be bound in a role it does
/// not declare. `kiosk` is a display and is bound to no resource. A device may
/// declare several: the point of the move away from a single `kind` is that one
/// unit can both read a card and switch power.
///
/// This is the Rust source of truth for the vocabulary; the SQL side is the
/// `space_devices_roles_vocab` CHECK, and
/// `checks/tests/device_capabilities_agree.rs` asserts the two agree.
pub mod device_role {
    pub const READER: &str = "reader";
    pub const POWER: &str = "power";
    pub const SENSOR: &str = "sensor";
    pub const EDGE: &str = "edge";
    pub const KIOSK: &str = "kiosk";
    pub const ALL: [&str; 5] = [READER, POWER, SENSOR, EDGE, KIOSK];
}

/// Device platform enum
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow,
)]
#[diesel(sql_type = sql_types::SpaceDevicePlatform)]
pub enum SpaceDevicePlatform {
    Windows,
    Linux,
    MacOs,
    Other,
}

// Implement Diesel traits for SpaceDevicePlatform enum
impl diesel::serialize::ToSql<sql_types::SpaceDevicePlatform, diesel::pg::Pg>
    for SpaceDevicePlatform
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        match self {
            SpaceDevicePlatform::Windows => out.write_all(b"windows")?,
            SpaceDevicePlatform::Linux => out.write_all(b"linux")?,
            SpaceDevicePlatform::MacOs => out.write_all(b"macos")?,
            SpaceDevicePlatform::Other => out.write_all(b"other")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::SpaceDevicePlatform, diesel::pg::Pg>
    for SpaceDevicePlatform
{
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"windows" => Ok(SpaceDevicePlatform::Windows),
            b"linux" => Ok(SpaceDevicePlatform::Linux),
            b"macos" => Ok(SpaceDevicePlatform::MacOs),
            b"other" => Ok(SpaceDevicePlatform::Other),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

/// Space Device - represents an Edge or Kiosk device
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = space_devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SpaceDevice {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub mac_address: String,
    pub software_version: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub uptime: i64,
    pub platform: SpaceDevicePlatform,
    pub place_id: Option<Uuid>,
    /// #101: the device's declared capabilities (roles it can fill + the
    /// firmware-enforcement descriptors). Replaces the single-valued `kind`.
    /// Positioned last to match the `ADD COLUMN` order the Queryable reads.
    pub capabilities: serde_json::Value,
}

/// New device for insertion
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = space_devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSpaceDevice {
    pub name: String,
    /// #101: declared capabilities blob (roles + enforcement descriptors),
    /// validated against `device_role` at the registration endpoint.
    pub capabilities: serde_json::Value,
    pub mac_address: String,
    pub software_version: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub uptime: i64,
    pub platform: SpaceDevicePlatform,
}

/// Update device fields
#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize, Default)]
#[diesel(table_name = space_devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateSpaceDevice {
    pub name: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub mac_address: Option<String>,
    pub software_version: Option<String>,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub uptime: Option<i64>,
    pub platform: Option<SpaceDevicePlatform>,
    pub updated_at: Option<DateTime<Utc>>,
    /// Outer Option = field present?; inner Option = SET NULL?
    pub place_id: Option<Option<Uuid>>,
}

/// Device authentication token
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = space_device_auth)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SpaceDeviceAuth {
    pub id: Uuid,
    pub device_id: Uuid,
    pub auth_token: String,
    pub created_at: DateTime<Utc>,
    /// #120 (#121): the device's command-channel HMAC key, sealed at rest.
    /// Appended last to keep the positional Queryable aligned with schema.rs.
    pub command_key_sealed: Option<Vec<u8>>,
    pub command_key_nonce: Option<Vec<u8>>,
}

/// New device auth for insertion
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = space_device_auth)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSpaceDeviceAuth {
    pub device_id: Uuid,
    pub auth_token: String,
    /// #120 (#121): sealed per-device command-channel HMAC key (+ its nonce).
    pub command_key_sealed: Option<Vec<u8>>,
    pub command_key_nonce: Option<Vec<u8>>,
}

/// Device authentication request (invite code)
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = space_device_auth_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SpaceDeviceAuthRequest {
    pub id: Uuid,
    pub device_code: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// New device auth request for insertion
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = space_device_auth_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSpaceDeviceAuthRequest {
    pub device_code: String,
    pub expires_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

impl SpaceDeviceAuthRequest {
    pub fn new_device_code() -> String {
        use rand::seq::SliceRandom;
        let emojis = [
            // Space & Tech
            "🚀", "🌟", "🎯", "🔥", "⚡", "🌈", "🎨", "🎪", "🎭", "🎸", "🎺", "🎷", "🥳", "🤖",
            "👾", "💎", "🔮", "🎲", "🏆", "🎖️", "🏅", "⭐", "💫", "✨", "🌙", "☀️", "🌊", "🏔️",
            // Animals
            "🦄", "🐙", "🦋", "🐶", "🐱", "🐭", "🐹", "🐰", "🦊", "🐻", "🐼", "🐨", "🐯", "🦁",
            "🐮", "🐷", "🐸", "🐵", "🐔", "🐧", "🐦", "🐤", "🐣", "🐥", "🦆", "🦅", "🦉", "🦇",
            "🐺", "🐗", "🐴", "🦓", "🦒", "🐘", "🦏", "🦛", "🐪", "🐫", "🦘", "🐊", "🐢", "🦎",
            "🐍", "🐲", "🐉", "🦕", "🦖", "🐳", "🐋", "🐬", "🐟", "🐠", "🐡", "🦈", "🐙", "🦑",
            "🦐", "🦞", "🦀", "🐚", "🦗", "🐛", "🦋", "🐌", "🐞", "🐜", "🕷️", "🕸️", "🦂",
            // Food & Drinks
            "🍕", "🍔", "🍰", "🎂", "☕", "🍎", "🍊", "🍋", "🍌", "🍉", "🍇", "🍓", "🍈", "🍒",
            "🍑", "🥭", "🍍", "🥥", "🥝", "🍅", "🍆", "🥑", "🥦", "🥬", "🥒", "🌶️", "🌽", "🥕",
            "🧄", "🧅", "🥔", "🍠", "🥐", "🍞", "🥖", "🥨", "🧀", "🥚", "🍳", "🧈", "🥞", "🧇",
            "🥓", "🍗", "🍖", "🌭", "🍟", "🍝", "🍜", "🍲", "🍛", "🍣", "🍱", "🥟", "🦪", "🍤",
            "🍙", "🍘", "🍥", "🥠", "🥮", "🍢", "🍡", "🍧", "🍨", "🍦", "🥧", "🧁", "🍮", "🍭",
            "🍬", "🍫", "🍿", "🍩", "🍪", "🌰", "🥜", "🍯", "🥛", "🍼", "🫖", "🍵", "🧃", "🥤",
            "🧋", "🍶", "🍾", "🍷", "🍸", "🍹", "🍺", "🍻", "🥂", "🥃", "🧊",
            // Nature & Objects
            "🌺", "🌸", "🌼", "🌻", "🌷", "🌹", "🥀", "🌾", "🌿", "🍀", "🍃", "🌱", "🌲", "🌳",
            "🌴", "🌵", "🌶️", "🍄", "🌰", "🐚", "🪨", "🌍", "🌎", "🌏", "🌕", "🌖", "🌗", "🌘",
            "🌑", "🌒", "🌓", "🌔", "⭐", "🌟", "💫", "✨", "☄️", "☀️", "🌤️", "⛅", "🌦️", "🌧️",
            "⛈️", "🌩️", "🌨️", "❄️", "☃️", "⛄", "🌬️", "💨",
        ];

        let mut rng = rand::thread_rng();
        (0..8)
            .map(|_| *emojis.choose(&mut rng).unwrap())
            .collect::<String>()
    }
}

/// #120 (#137): the storage/wire form of a device invite code -- the code's
/// UTF-8 bytes as lowercase hex. That is pure ASCII, so it stores on any
/// database encoding; the codes themselves are emoji, which a non-Unicode
/// cluster (e.g. LATIN1) cannot hold. Encode before persisting or matching a
/// code; decode only to show the emoji back to an operator.
pub fn encode_device_code(display: &str) -> String {
    hex::encode(display.as_bytes())
}

/// Inverse of [`encode_device_code`]. Returns the input unchanged when it is not
/// valid hex-of-UTF-8 -- e.g. a legacy raw-emoji row on a UTF-8 cluster that
/// predates the migration -- so display never breaks.
pub fn decode_device_code(stored: &str) -> String {
    hex::decode(stored)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| stored.to_string())
}
