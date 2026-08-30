//! `css-cli` — the administrative and management client for a CSS instance.
//!
//! The modules live here and both binaries are shims over them. Until this
//! split there was no lib target at all, so `cli/tests/` could not reach a
//! single function and the only way to exercise anything was to run the binary
//! as a subprocess. It also meant every `pub fn` the binary did not itself call
//! — `output::print_info`, the whole `commands::admin` module — was reported as
//! dead, because a binary's items are dead unless the binary uses them.

pub mod auth;
pub mod client;
pub mod commands;
pub mod config;
pub mod output;
