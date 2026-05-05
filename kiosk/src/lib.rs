use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum KioskTheme {
    Her,
    AfterDark,
    Forest,
    Sky,
    Clays,
    #[default]
    Stones,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum KioskDisplayMode {
    /// Manual switching only — Space bar toggles between views.
    Static,
    /// Automatic rotation every `s` seconds. Space bar also toggles.
    Automatic { s: usize },
    /// Both views displayed side by side (left: ToolGuard, right: Calendar).
    HorizontalPanes,
    /// Both views stacked (top: ToolGuard, bottom: Calendar).
    VerticalPanes,
}

impl Default for KioskDisplayMode {
    fn default() -> Self {
        KioskDisplayMode::Static
    }
}
