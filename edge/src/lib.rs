//! `css-edge` — the on-site agent that talks to the CSS server and to local
//! hardware (RFID readers, tool relays, kiosks).
//!
//! Everything lives in the library and the binary is a thin shim over it. That
//! is not cosmetic: until this split, all ten modules were declared in
//! `main.rs`, so nothing here could be reached from `edge/tests/` and the only
//! testable surface was whatever a module could assert about itself from the
//! inside. It also meant a large amount of genuinely-live code —
//! [`toolguard::ToolGuardState::has_state`], [`doors::DoorsState::len`],
//! [`config::ConfigManager`] — was reported as dead, because a binary's private
//! items are dead unless the binary itself calls them.

pub mod calendar;
pub mod config;
pub mod doors;
pub mod edge_inbound;
pub mod mqtt;
pub mod registration;
pub mod static_files;
pub mod system_info;
pub mod toolguard;
pub mod web_server;
pub mod ws;

pub use static_files::StaticFileServiceEdge;
