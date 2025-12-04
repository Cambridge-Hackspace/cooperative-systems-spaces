use chrono::{DateTime, Utc};
use uuid::Uuid;

pub enum SpaceDeviceKind {
    Edge,
    Kiosk
}

pub enum SpaceDevicePlatform {
    Windows,
    Linux,
    MacOs,
    Other,
}


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
    pub uptime: usize,
    pub platform: SpaceDevicePlatform,
    // pub config: SpaceDeviceConfig,
}

pub struct SpaceDeviceAuth {
    pub id: Uuid,
    pub device_id: Uuid,
    pub auth_token: String,
}

pub struct SpaceDeviceAuthRequest {
    pub device_code: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

impl Default for SpaceDeviceAuthRequest {
    fn default() -> Self {
        Self {
            device_code: "".to_string(),
            expires_at: Utc::now(),
            used_at: None,
        }
    }
}
impl SpaceDeviceAuthRequest {
    fn new_device_code() -> String {
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
            .map(|_| emojis.choose(&mut rng).unwrap())
            .collect::<String>()
    }
}