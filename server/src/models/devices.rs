use crate::schema::{space_device_auth, space_device_auth_requests, space_devices, sql_types};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

/// Device kind: the coordinator/display roles, plus the tool access modules.
///
/// `Edge` is the local coordinator and `Kiosk` a display. `CardReader`,
/// `PowerController` and `Sensor` are tool access modules (#83) -- separate
/// physical units bound to a tool through `tool_modules`, rather than
/// peripherals of whichever edge happens to host them.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow,
)]
#[diesel(sql_type = sql_types::SpaceDeviceKind)]
pub enum SpaceDeviceKind {
    Edge,
    Kiosk,
    CardReader,
    PowerController,
    Sensor,
}

// Implement Diesel traits for SpaceDeviceKind enum
impl diesel::serialize::ToSql<sql_types::SpaceDeviceKind, diesel::pg::Pg> for SpaceDeviceKind {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        match self {
            SpaceDeviceKind::Edge => out.write_all(b"edge")?,
            SpaceDeviceKind::Kiosk => out.write_all(b"kiosk")?,
            SpaceDeviceKind::CardReader => out.write_all(b"card_reader")?,
            SpaceDeviceKind::PowerController => out.write_all(b"power_controller")?,
            SpaceDeviceKind::Sensor => out.write_all(b"sensor")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::SpaceDeviceKind, diesel::pg::Pg> for SpaceDeviceKind {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"edge" => Ok(SpaceDeviceKind::Edge),
            b"kiosk" => Ok(SpaceDeviceKind::Kiosk),
            b"card_reader" => Ok(SpaceDeviceKind::CardReader),
            b"power_controller" => Ok(SpaceDeviceKind::PowerController),
            b"sensor" => Ok(SpaceDeviceKind::Sensor),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
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
    pub kind: SpaceDeviceKind,
    pub mac_address: String,
    pub software_version: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub uptime: i64,
    pub platform: SpaceDevicePlatform,
    pub place_id: Option<Uuid>,
}

/// New device for insertion
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = space_devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSpaceDevice {
    pub name: String,
    pub kind: SpaceDeviceKind,
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
