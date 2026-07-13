use dashmap::DashMap;
use sqlx::PgPool;
use twilight_model::id::{Id, marker::GuildMarker};

use crate::storage::models::{self, GuildSettings};

/// Caches `guild_settings` in memory so the message-handling hot path —
/// checked on every single message, join, and audit-log entry — never
/// makes a network round-trip to Supabase just to read a handful of
/// booleans and thresholds that almost never change.
///
/// Populated lazily on first read per guild. `/config` (`discord::config`)
/// is the only thing that writes to `guild_settings` after its first-time
/// creation by `upsert_guild_config`, and it calls `invalidate` right
/// after every successful write, so a changed threshold is never served
/// stale.
#[derive(Default)]
pub struct SettingsCache {
    entries: DashMap<Id<GuildMarker>, GuildSettings>,
}

impl SettingsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, pool: &PgPool, guild_id: Id<GuildMarker>) -> GuildSettings {
        if let Some(cached) = self.entries.get(&guild_id) {
            return cached.clone();
        }

        let settings = models::get_guild_settings(pool, guild_id).await;
        self.entries.insert(guild_id, settings.clone());
        settings
    }

    /// Drops a guild's cached settings so the next `get` re-reads from
    /// Postgres. Call this right after any write to that guild's
    /// `guild_settings` row.
    pub fn invalidate(&self, guild_id: Id<GuildMarker>) {
        self.entries.remove(&guild_id);
    }
}
