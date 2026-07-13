//! Everything that talks directly to Discord: the REST client, slash
//! command definitions/registration, and interaction (slash command)
//! handling. Gateway *event* handling (messages, joins, etc.) lives in
//! `gateway.rs` at the crate root, one level up.

pub mod backup;
pub mod commands;
pub mod config;
pub mod embeds;
pub mod http;
pub mod interactions;
pub mod lockdown;
pub mod security_score;
pub mod setup;
pub mod verify_panel;
