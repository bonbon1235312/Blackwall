use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use sqlx::PgPool;
use twilight_http::Client as HttpClient;
use twilight_model::guild::Permissions;
use twilight_model::id::marker::{GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;

use crate::storage::models;

/// A guild's role-permission table plus its owner, everything
/// `PermissionCalculator` needs to compute an effective permission set,
/// without a fresh Discord API call per computation.
pub struct GuildRoleSnapshot {
    pub everyone_permissions: Permissions,
    pub roles: HashMap<Id<RoleMarker>, Permissions>,
    pub owner_id: Option<Id<UserMarker>>,
}

/// Caches each guild's role-permission table so prefix-command permission
/// checks (`CommandSource::invoker_permissions`) don't pay a
/// `state.http.roles(guild_id)` round-trip on every single `!`/`?`
/// invocation. Same lazy-populate-plus-event-invalidate shape as
/// `SettingsCache` — populated on first use per guild, kept fresh by
/// `gateway.rs`'s `RoleCreate`/`RoleUpdate`/`RoleDelete` handlers calling
/// `invalidate` rather than by re-fetching on every read.
///
/// A member's own role *assignments* don't need caching here — Discord
/// already attaches them inline on every `Message.member`/`Interaction`,
/// no separate API call was ever needed for that half.
#[derive(Default)]
pub struct RoleCache {
    entries: DashMap<Id<GuildMarker>, Arc<GuildRoleSnapshot>>,
}

impl RoleCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `None` only if the guild's roles couldn't be read at all
    /// (Discord error, or the bot was removed from the guild) — callers
    /// should treat that the same as "no permissions" rather than panic.
    pub async fn get(
        &self,
        http: &HttpClient,
        db: &PgPool,
        guild_id: Id<GuildMarker>,
    ) -> Option<Arc<GuildRoleSnapshot>> {
        if let Some(cached) = self.entries.get(&guild_id) {
            return Some(Arc::clone(&cached));
        }

        let roles = http.roles(guild_id).await.ok()?.model().await.ok()?;

        let everyone_permissions = roles
            .iter()
            .find(|role| role.id.get() == guild_id.get())
            .map_or_else(Permissions::empty, |role| role.permissions);

        let role_permissions = roles.iter().map(|role| (role.id, role.permissions)).collect();

        // Same owner-lookup fallback `CommandSource::invoker_permissions`
        // used to do inline: prefer the recorded owner (set by `/setup`),
        // fall back to asking Discord directly for a guild Blackwall
        // hasn't recorded yet.
        let owner_id = match models::get_owner_id(db, guild_id).await {
            Some(owner_id) => Some(owner_id),
            None => http
                .guild(guild_id)
                .await
                .ok()?
                .model()
                .await
                .ok()
                .map(|guild| guild.owner_id),
        };

        let snapshot = Arc::new(GuildRoleSnapshot {
            everyone_permissions,
            roles: role_permissions,
            owner_id,
        });
        self.entries.insert(guild_id, Arc::clone(&snapshot));
        Some(snapshot)
    }

    /// Drops a guild's cached role table so the next `get` re-fetches from
    /// Discord. Call this from `RoleCreate`/`RoleUpdate`/`RoleDelete`.
    pub fn invalidate(&self, guild_id: Id<GuildMarker>) {
        self.entries.remove(&guild_id);
    }
}
