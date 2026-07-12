use dashmap::DashMap;
use sqlx::PgPool;
use twilight_model::id::{Id, marker::GuildMarker};

use crate::storage::models::{self, GuildSettings};

/// Caches `guild_settings` in memory so the message-handling hot path —
/// checked on every single message, join, and audit-log entry — never
/// makes a network round-trip to Supabase just to read a handful of
/// booleans that almost never change.
///
/// Populated lazily on first read per guild. Nothing currently writes to
/// `guild_settings` after its first-time creation by
/// `upsert_guild_config` (that doesn't need an invalidation either, since
/// a freshly-created row's values are exactly what `get_guild_settings`
/// already falls back to for a missing row) — if a future `/config`-style
/// command adds a way to flip these toggles, that's the point to add an
/// `invalidate(guild_id)` method back here.
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
}
