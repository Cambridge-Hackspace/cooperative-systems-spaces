use chrono::{DateTime, Utc};
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use diesel::prelude::*;
use std::io::Write;
use crate::schema::{space_devices, space_device_auth, space_device_auth_requests, sql_types};

/// Device kind enum - Edge or Kiosk
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow)]
#[diesel(sql_type = sql_types::SpaceDeviceKind)]
pub enum SpaceDeviceKind {
    Edge,
    Kiosk
}

// Implement Diesel traits for SpaceDeviceKind enum
impl diesel::serialize::ToSql<sql_types::SpaceDeviceKind, diesel::pg::Pg> for SpaceDeviceKind {
    fn to_sql<'b>(&'b self, out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>) -> diesel::serialize::Result {
        match self {
            SpaceDeviceKind::Edge => out.write_all(b"edge")?,
            SpaceDeviceKind::Kiosk => out.write_all(b"kiosk")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::SpaceDeviceKind, diesel::pg::Pg> for SpaceDeviceKind {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"edge" => Ok(SpaceDeviceKind::Edge),
            b"kiosk" => Ok(SpaceDeviceKind::Kiosk),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

/// Device platform enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow)]
#[diesel(sql_type = sql_types::SpaceDevicePlatform)]
pub enum SpaceDevicePlatform {
    Windows,
    Linux,
    MacOs,
    Other,
}

// Implement Diesel traits for SpaceDevicePlatform enum
impl diesel::serialize::ToSql<sql_types::SpaceDevicePlatform, diesel::pg::Pg> for SpaceDevicePlatform {
    fn to_sql<'b>(&'b self, out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>) -> diesel::serialize::Result {
        match self {
            SpaceDevicePlatform::Windows => out.write_all(b"windows")?,
            SpaceDevicePlatform::Linux => out.write_all(b"linux")?,
            SpaceDevicePlatform::MacOs => out.write_all(b"macos")?,
            SpaceDevicePlatform::Other => out.write_all(b"other")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::SpaceDevicePlatform, diesel::pg::Pg> for SpaceDevicePlatform {
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
}

/// New device auth for insertion
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = space_device_auth)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSpaceDeviceAuth {
    pub device_id: Uuid,
    pub auth_token: String,
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
            "🚀", "🌟", "🎯", "🔥", "⚡", "🌈", "🎨", "🎪", "🎭", "🎸",
            "🎺", "🎷", "🥳", "🤖", "👾", "💎", "🔮", "🎲", "🏆", "🎖️",
            "🏅", "⭐", "💫", "✨", "🌙", "☀️", "🌊", "🏔️",

            // Animals
            "🦄", "🐙", "🦋", "🐶", "🐱", "🐭", "🐹", "🐰", "🦊", "🐻",
            "🐼", "🐨", "🐯", "🦁", "🐮", "🐷", "🐸", "🐵", "🐔", "🐧",
            "🐦", "🐤", "🐣", "🐥", "🦆", "🦅", "🦉", "🦇", "🐺", "🐗",
            "🐴", "🦓", "🦒", "🐘", "🦏", "🦛", "🐪", "🐫", "🦘", "🐊",
            "🐢", "🦎", "🐍", "🐲", "🐉", "🦕", "🦖", "🐳", "🐋", "🐬",
            "🐟", "🐠", "🐡", "🦈", "🐙", "🦑", "🦐", "🦞", "🦀", "🐚",
            "🦗", "🐛", "🦋", "🐌", "🐞", "🐜", "🕷️", "🕸️", "🦂",

            // Food & Drinks
            "🍕", "🍔", "🍰", "🎂", "☕", "🍎", "🍊", "🍋", "🍌", "🍉",
            "🍇", "🍓", "🍈", "🍒", "🍑", "🥭", "🍍", "🥥", "🥝", "🍅",
            "🍆", "🥑", "🥦", "🥬", "🥒", "🌶️", "🌽", "🥕", "🧄", "🧅",
            "🥔", "🍠", "🥐", "🍞", "🥖", "🥨", "🧀", "🥚", "🍳", "🧈",
            "🥞", "🧇", "🥓", "🍗", "🍖", "🌭", "🍟", "🍝", "🍜", "🍲",
            "🍛", "🍣", "🍱", "🥟", "🦪", "🍤", "🍙", "🍘", "🍥", "🥠",
            "🥮", "🍢", "🍡", "🍧", "🍨", "🍦", "🥧", "🧁", "🍮", "🍭",
            "🍬", "🍫", "🍿", "🍩", "🍪", "🌰", "🥜", "🍯", "🥛", "🍼",
            "🫖", "🍵", "🧃", "🥤", "🧋", "🍶", "🍾", "🍷", "🍸", "🍹",
            "🍺", "🍻", "🥂", "🥃", "🧊",

            // Nature & Objects
            "🌺", "🌸", "🌼", "🌻", "🌷", "🌹", "🥀", "🌾", "🌿", "🍀",
            "🍃", "🌱", "🌲", "🌳", "🌴", "🌵", "🌶️", "🍄", "🌰", "🐚",
            "🪨", "🌍", "🌎", "🌏", "🌕", "🌖", "🌗", "🌘", "🌑", "🌒",
            "🌓", "🌔", "⭐", "🌟", "💫", "✨", "☄️", "☀️", "🌤️", "⛅",
            "🌦️", "🌧️", "⛈️", "🌩️", "🌨️", "❄️", "☃️", "⛄", "🌬️", "💨"
        ];

        let mut rng = rand::thread_rng();
        (0..8)
            .map(|_| *emojis.choose(&mut rng).unwrap())
            .collect::<String>()
    }
}