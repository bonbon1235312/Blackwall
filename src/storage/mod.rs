//! Persistence: everything Blackwall remembers between restarts, keyed by
//! guild ID so each server gets its own configuration instead of sharing
//! one global setup.

pub mod database;
pub mod models;
