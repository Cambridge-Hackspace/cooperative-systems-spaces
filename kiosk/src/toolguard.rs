use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Wire types (mirror edge/src/toolguard.rs) ─────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Idle,
    InUse,
    Maintenance,
    Broken,
    Repair,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTool {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncUser {
    pub full_name: String,
    pub authorized_tool_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub tools: Vec<SyncTool>,
    pub users: Vec<SyncUser>,
}

// ── Display model ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ToolDisplay {
    pub name: String,
    pub status: ToolStatus,
    /// Populated when status == InUse; mirrors toolguard-status-ui logic.
    pub inuse_by: Option<String>,
}

pub fn tools_from_payload(payload: &SyncPayload) -> Vec<ToolDisplay> {
    let mut inuse_by: HashMap<String, String> = HashMap::new();
    for user in &payload.users {
        for tool in &payload.tools {
            if tool.status == ToolStatus::InUse && user.authorized_tool_ids.contains(&tool.id) {
                inuse_by
                    .entry(tool.id.clone())
                    .or_insert_with(|| user.full_name.clone());
            }
        }
    }

    let mut tools: Vec<ToolDisplay> = payload
        .tools
        .iter()
        .map(|t| ToolDisplay {
            name: t.name.clone(),
            status: t.status.clone(),
            inuse_by: inuse_by.get(&t.id).cloned(),
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

pub fn status_color(status: &ToolStatus) -> Color {
    match status {
        ToolStatus::Idle => Color::srgb_u8(50, 200, 80),
        ToolStatus::InUse => Color::srgb_u8(50, 150, 230),
        ToolStatus::Maintenance => Color::srgb_u8(230, 190, 40),
        ToolStatus::Repair => Color::srgb_u8(230, 130, 40),
        ToolStatus::Broken => Color::srgb_u8(220, 60, 60),
        ToolStatus::Retired => Color::srgb_u8(120, 120, 120),
    }
}

pub fn status_label(status: &ToolStatus) -> &'static str {
    match status {
        ToolStatus::Idle => "Idle",
        ToolStatus::InUse => "In Use",
        ToolStatus::Maintenance => "Maintenance",
        ToolStatus::Repair => "Repair",
        ToolStatus::Broken => "Broken",
        ToolStatus::Retired => "Retired",
    }
}

// ── Bevy resource ─────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
pub struct ToolGuardData {
    pub tools: Vec<ToolDisplay>,
    pub generation: u32,
}

impl ToolGuardData {
    pub fn update(&mut self, payload: SyncPayload) {
        self.tools = tools_from_payload(&payload);
        self.generation = self.generation.wrapping_add(1);
    }
}
