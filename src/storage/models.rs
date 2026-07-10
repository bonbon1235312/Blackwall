use sqlx::{Row, SqlitePool};
use twilight_model::guild::Permissions;
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};

/// A server's Blackwall configuration, mirroring a row in the `guilds`
/// table.
pub struct GuildConfig {
    pub guild_id: Id<GuildMarker>,
    pub owner_id: Id<UserMarker>,
    pub log_channel_id: Option<Id<ChannelMarker>>,
    pub verified_role_id: Option<Id<RoleMarker>>,
    pub quarantine_role_id: Option<Id<RoleMarker>>,
}

/// Creates or updates a guild's configuration, and makes sure a
/// `guild_settings` row exists for it with sensible defaults.
///
/// Existing feature toggles in `guild_settings` are never touched by this:
/// it only fills them in the first time, so changing a channel or role in
/// the setup panel cannot silently undo a setting an admin already changed.
pub async fn upsert_guild_config(
    pool: &SqlitePool,
    config: &GuildConfig,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO guilds (guild_id, owner_id, log_channel_id, verified_role_id, quarantine_role_id)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(guild_id) DO UPDATE SET
            owner_id = excluded.owner_id,
            log_channel_id = excluded.log_channel_id,
            verified_role_id = excluded.verified_role_id,
            quarantine_role_id = excluded.quarantine_role_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(config.guild_id.to_string())
    .bind(config.owner_id.to_string())
    .bind(config.log_channel_id.map(|id| id.to_string()))
    .bind(config.verified_role_id.map(|id| id.to_string()))
    .bind(config.quarantine_role_id.map(|id| id.to_string()))
    .execute(pool)
    .await?;

    sqlx::query("INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?)")
        .bind(config.guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Looks up a guild's saved setup choices, if it has been initialized.
pub async fn get_guild_config(pool: &SqlitePool, guild_id: Id<GuildMarker>) -> Option<GuildConfig> {
    let row = sqlx::query(
        "SELECT owner_id, log_channel_id, verified_role_id, quarantine_role_id
         FROM guilds WHERE guild_id = ?",
    )
    .bind(guild_id.to_string())
    .fetch_optional(pool)
    .await
    .expect("failed to look up guild config")?;

    let owner_id: String = row
        .try_get("owner_id")
        .expect("guilds.owner_id column missing or the wrong type");
    let log_channel_id: Option<String> = row
        .try_get("log_channel_id")
        .expect("guilds.log_channel_id column missing or the wrong type");
    let verified_role_id: Option<String> = row
        .try_get("verified_role_id")
        .expect("guilds.verified_role_id column missing or the wrong type");
    let quarantine_role_id: Option<String> = row
        .try_get("quarantine_role_id")
        .expect("guilds.quarantine_role_id column missing or the wrong type");

    Some(GuildConfig {
        guild_id,
        owner_id: owner_id
            .parse()
            .expect("owner_id stored in the database was not a valid Discord ID"),
        log_channel_id: log_channel_id.map(|raw| {
            raw.parse()
                .expect("log_channel_id stored in the database was not a valid Discord ID")
        }),
        verified_role_id: verified_role_id.map(|raw| {
            raw.parse()
                .expect("verified_role_id stored in the database was not a valid Discord ID")
        }),
        quarantine_role_id: quarantine_role_id.map(|raw| {
            raw.parse()
                .expect("quarantine_role_id stored in the database was not a valid Discord ID")
        }),
    })
}

/// Updates the log channel selected in the setup panel.
pub async fn set_log_channel_id(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE guilds SET log_channel_id = ?, updated_at = CURRENT_TIMESTAMP WHERE guild_id = ?",
    )
    .bind(channel_id.to_string())
    .bind(guild_id.to_string())
    .execute(pool)
    .await?;

    Ok(())
}

/// Updates the role granted after a successful OAuth verification.
pub async fn set_verified_role_id(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE guilds SET verified_role_id = ?, updated_at = CURRENT_TIMESTAMP WHERE guild_id = ?",
    )
    .bind(role_id.to_string())
    .bind(guild_id.to_string())
    .execute(pool)
    .await?;

    Ok(())
}

/// Updates the role used to quarantine suspicious members.
pub async fn set_quarantine_role_id(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE guilds SET quarantine_role_id = ?, updated_at = CURRENT_TIMESTAMP WHERE guild_id = ?",
    )
    .bind(role_id.to_string())
    .bind(guild_id.to_string())
    .execute(pool)
    .await?;

    Ok(())
}

/// A guild's feature toggles, mirroring a row in the `guild_settings`
/// table. Only the fields something actually reads are included here —
/// the rest of the columns get their own field once the feature that uses
/// them exists (verification).
pub struct GuildSettings {
    pub anti_spam_enabled: bool,
    pub anti_scam_enabled: bool,
    pub anti_raid_enabled: bool,
    pub anti_nuke_enabled: bool,
    pub support_join_enabled: bool,
}

/// Looks up a guild's feature toggles.
///
/// A guild that hasn't run `/setup` yet has no `guild_settings` row at
/// all — `anti_spam`/`anti_scam`/`anti_raid`/`anti_nuke` are treated as
/// "on" in that case, matching the smart defaults `/setup` itself would
/// have saved. `support_join` is the one exception: it's opt-in, so a
/// missing row (or a row that's never had it explicitly turned on) means
/// "off," matching the column's own `DEFAULT 0`.
pub async fn get_guild_settings(pool: &SqlitePool, guild_id: Id<GuildMarker>) -> GuildSettings {
    let row = sqlx::query(
        "SELECT anti_spam_enabled, anti_scam_enabled, anti_raid_enabled, anti_nuke_enabled,
                support_join_enabled
         FROM guild_settings WHERE guild_id = ?",
    )
    .bind(guild_id.to_string())
    .fetch_optional(pool)
    .await
    .expect("failed to look up guild settings");

    let Some(row) = row else {
        return GuildSettings {
            anti_spam_enabled: true,
            anti_scam_enabled: true,
            anti_raid_enabled: true,
            anti_nuke_enabled: true,
            support_join_enabled: false,
        };
    };

    GuildSettings {
        anti_spam_enabled: row
            .try_get("anti_spam_enabled")
            .expect("guild_settings.anti_spam_enabled column missing or the wrong type"),
        anti_scam_enabled: row
            .try_get("anti_scam_enabled")
            .expect("guild_settings.anti_scam_enabled column missing or the wrong type"),
        anti_raid_enabled: row
            .try_get("anti_raid_enabled")
            .expect("guild_settings.anti_raid_enabled column missing or the wrong type"),
        anti_nuke_enabled: row
            .try_get("anti_nuke_enabled")
            .expect("guild_settings.anti_nuke_enabled column missing or the wrong type"),
        support_join_enabled: row
            .try_get("support_join_enabled")
            .expect("guild_settings.support_join_enabled column missing or the wrong type"),
    }
}

/// Turns the per-guild support-server-join opt-in on or off.
///
/// Called from `/setup`'s optional `support_server_join` option. Assumes
/// the `guild_settings` row already exists (guaranteed by
/// `upsert_guild_config`, which always runs first in `/setup`).
pub async fn set_support_join_enabled(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    enabled: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE guild_settings SET support_join_enabled = ? WHERE guild_id = ?")
        .bind(enabled)
        .bind(guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Looks up where a guild's moderation logs should be sent, if configured.
///
/// This runs on every message a moderation filter acts on, so it stays a
/// single, simple lookup rather than something heavier.
pub async fn get_log_channel_id(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> Option<Id<ChannelMarker>> {
    let row = sqlx::query("SELECT log_channel_id FROM guilds WHERE guild_id = ?")
        .bind(guild_id.to_string())
        .fetch_optional(pool)
        .await
        .expect("failed to look up a guild's log channel")?;

    let raw: Option<String> = row
        .try_get("log_channel_id")
        .expect("guilds.log_channel_id column missing or the wrong type");

    raw.map(|raw| {
        raw.parse()
            .expect("log_channel_id stored in the database was not a valid Discord ID")
    })
}

/// Looks up a guild's owner, as recorded the last time `/setup` ran.
///
/// Discord never allows any bot, regardless of its own permissions or
/// role position, to moderate (timeout/kick/ban) the guild owner — it's
/// a hard rule baked into the API, not something a permission grant can
/// unlock. Checking this before attempting a moderation action avoids a
/// guaranteed-to-fail API call and the resulting noisy error log.
///
/// Returns `None` for a guild that hasn't run `/setup` yet — callers
/// should treat "unknown owner" the same as "not the owner" and still
/// attempt the action, since skipping it on a guess would be wrong too
/// often to be worth it.
pub async fn get_owner_id(pool: &SqlitePool, guild_id: Id<GuildMarker>) -> Option<Id<UserMarker>> {
    let row = sqlx::query("SELECT owner_id FROM guilds WHERE guild_id = ?")
        .bind(guild_id.to_string())
        .fetch_optional(pool)
        .await
        .expect("failed to look up a guild's owner")?;

    let raw: String = row
        .try_get("owner_id")
        .expect("guilds.owner_id column missing or the wrong type");

    Some(
        raw.parse()
            .expect("owner_id stored in the database was not a valid Discord ID"),
    )
}

/// Every guild Blackwall manages (has a `/setup` row for) whose owner is
/// `user_id` — the owner dashboard's entire access-control mechanism.
/// Deliberately reads from Blackwall's own data rather than asking
/// Discord which servers the logged-in user is in: it's already recorded
/// here, and avoids needing the `guilds` OAuth scope at all for the
/// dashboard login (see `verification::oauth::dashboard_authorize_url`).
pub async fn get_guilds_owned_by(
    pool: &SqlitePool,
    user_id: Id<UserMarker>,
) -> Vec<Id<GuildMarker>> {
    let rows = sqlx::query("SELECT guild_id FROM guilds WHERE owner_id = ?")
        .bind(user_id.to_string())
        .fetch_all(pool)
        .await
        .expect("failed to look up guilds owned by a user");

    rows.into_iter()
        .map(|row| {
            let raw: String = row
                .try_get("guild_id")
                .expect("guilds.guild_id column missing or the wrong type");
            raw.parse()
                .expect("guild_id stored in the database was not a valid Discord ID")
        })
        .collect()
}

/// Looks up the role configured by `/setup` for successful verification.
pub async fn get_verified_role_id(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> Result<Option<Id<RoleMarker>>, sqlx::Error> {
    let Some(row) = sqlx::query("SELECT verified_role_id FROM guilds WHERE guild_id = ?")
        .bind(guild_id.to_string())
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };

    let raw: Option<String> = row.try_get("verified_role_id")?;

    Ok(raw.map(|raw| {
        raw.parse()
            .expect("verified_role_id stored in the database was not a valid Discord ID")
    }))
}

/// Looks up the role configured by `/setup` for quarantining suspicious
/// or nuke-detected members.
pub async fn get_quarantine_role_id(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> Option<Id<RoleMarker>> {
    let row = sqlx::query("SELECT quarantine_role_id FROM guilds WHERE guild_id = ?")
        .bind(guild_id.to_string())
        .fetch_optional(pool)
        .await
        .expect("failed to look up a guild's quarantine role")?;

    let raw: Option<String> = row
        .try_get("quarantine_role_id")
        .expect("guilds.quarantine_role_id column missing or the wrong type");

    raw.map(|raw| {
        raw.parse()
            .expect("quarantine_role_id stored in the database was not a valid Discord ID")
    })
}

/// Records that a user successfully completed verification for a guild.
pub async fn record_verification(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    method: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR REPLACE INTO verified_users (guild_id, user_id, method)
         VALUES (?, ?, ?)",
    )
    .bind(guild_id.to_string())
    .bind(user_id.to_string())
    .bind(method)
    .execute(pool)
    .await?;

    Ok(())
}

/// Records a moderation/security event for later dashboard and audit views.
pub async fn record_security_event(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    user_id: Option<Id<UserMarker>>,
    event_type: &str,
    severity: &str,
    description: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO security_events (guild_id, user_id, event_type, severity, description)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(guild_id.to_string())
    .bind(user_id.map(|id| id.to_string()))
    .bind(event_type)
    .bind(severity)
    .bind(description)
    .execute(pool)
    .await?;

    Ok(())
}

/// One row from `security_events`, as shown on the owner dashboard.
pub struct SecurityEvent {
    pub event_type: String,
    pub severity: String,
    pub description: String,
    pub created_at: String,
}

/// The most recent security events for a guild, newest first — the
/// dashboard's "recent incidents" list.
pub async fn get_recent_security_events(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    limit: i64,
) -> Vec<SecurityEvent> {
    let rows = sqlx::query(
        "SELECT event_type, severity, description, created_at FROM security_events
         WHERE guild_id = ? ORDER BY id DESC LIMIT ?",
    )
    .bind(guild_id.to_string())
    .bind(limit)
    .fetch_all(pool)
    .await
    .expect("failed to look up recent security events");

    rows.into_iter()
        .map(|row| SecurityEvent {
            event_type: row
                .try_get("event_type")
                .expect("security_events.event_type column missing or the wrong type"),
            severity: row
                .try_get("severity")
                .expect("security_events.severity column missing or the wrong type"),
            description: row
                .try_get("description")
                .expect("security_events.description column missing or the wrong type"),
            created_at: row
                .try_get("created_at")
                .expect("security_events.created_at column missing or the wrong type"),
        })
        .collect()
}

/// A channel's pre-lockdown `@everyone` overwrite state, as saved by
/// `actions::lockdown::engage` and consumed by `actions::lockdown::revert`.
pub struct LockdownSnapshot {
    pub channel_id: Id<ChannelMarker>,
    /// `false` means the channel had no `@everyone` overwrite at all
    /// before lockdown — restoring means deleting the overwrite Blackwall
    /// added, not writing back all-zero permission bits.
    pub had_overwrite: bool,
    pub everyone_allow: Permissions,
    pub everyone_deny: Permissions,
}

/// Saves one channel's pre-lockdown `@everyone` overwrite state.
///
/// `existing` is `None` if the channel had no `@everyone` overwrite
/// before lockdown started.
pub async fn save_lockdown_snapshot(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    existing: Option<(Permissions, Permissions)>,
) {
    let (had_overwrite, allow, deny) = match existing {
        Some((allow, deny)) => (true, Some(allow.bits() as i64), Some(deny.bits() as i64)),
        None => (false, None, None),
    };

    sqlx::query(
        "INSERT OR REPLACE INTO lockdown_snapshots
            (guild_id, channel_id, had_overwrite, everyone_allow, everyone_deny)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(guild_id.to_string())
    .bind(channel_id.to_string())
    .bind(had_overwrite)
    .bind(allow)
    .bind(deny)
    .execute(pool)
    .await
    .expect("failed to save a lockdown snapshot");
}

/// Reads back every lockdown snapshot for a guild and deletes them — this
/// is meant to be called exactly once, by `/unlockdown`, as the one-time
/// "restore everything, then forget it" step. Calling it again on a guild
/// that isn't locked down just returns an empty list.
pub async fn take_lockdown_snapshots(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> Vec<LockdownSnapshot> {
    let rows = sqlx::query(
        "SELECT channel_id, had_overwrite, everyone_allow, everyone_deny
         FROM lockdown_snapshots WHERE guild_id = ?",
    )
    .bind(guild_id.to_string())
    .fetch_all(pool)
    .await
    .expect("failed to read lockdown snapshots");

    sqlx::query("DELETE FROM lockdown_snapshots WHERE guild_id = ?")
        .bind(guild_id.to_string())
        .execute(pool)
        .await
        .expect("failed to clear lockdown snapshots");

    rows.into_iter()
        .map(|row| {
            let channel_id: String = row
                .try_get("channel_id")
                .expect("lockdown_snapshots.channel_id column missing or the wrong type");
            let had_overwrite: bool = row
                .try_get("had_overwrite")
                .expect("lockdown_snapshots.had_overwrite column missing or the wrong type");
            let allow_bits: Option<i64> = row
                .try_get("everyone_allow")
                .expect("lockdown_snapshots.everyone_allow column missing or the wrong type");
            let deny_bits: Option<i64> = row
                .try_get("everyone_deny")
                .expect("lockdown_snapshots.everyone_deny column missing or the wrong type");

            LockdownSnapshot {
                channel_id: channel_id
                    .parse()
                    .expect("channel_id stored in the database was not a valid Discord ID"),
                had_overwrite,
                everyone_allow: Permissions::from_bits_truncate(allow_bits.unwrap_or(0) as u64),
                everyone_deny: Permissions::from_bits_truncate(deny_bits.unwrap_or(0) as u64),
            }
        })
        .collect()
}

/// Sets `guilds.lockdown_enabled`. A no-op (affects zero rows) if the
/// guild hasn't run `/setup` yet — the lockdown itself (the channel
/// overwrites) still applies regardless; this flag is just for display.
pub async fn set_lockdown_enabled(pool: &SqlitePool, guild_id: Id<GuildMarker>, enabled: bool) {
    sqlx::query("UPDATE guilds SET lockdown_enabled = ? WHERE guild_id = ?")
        .bind(enabled)
        .bind(guild_id.to_string())
        .execute(pool)
        .await
        .expect("failed to update lockdown_enabled");
}

/// Saves a `/backup` snapshot. `backup_json` is a serialized
/// `discord::backup::GuildBackup` — storage doesn't know or care about its
/// shape, just that it's a string to keep and hand back later.
pub async fn create_backup(pool: &SqlitePool, guild_id: Id<GuildMarker>, backup_json: &str) {
    sqlx::query("INSERT INTO backups (guild_id, backup_json) VALUES (?, ?)")
        .bind(guild_id.to_string())
        .bind(backup_json)
        .execute(pool)
        .await
        .expect("failed to save backup");
}

/// Reads back the most recent backup's raw JSON for a guild, if one
/// exists. `/restore` only ever restores from the latest snapshot for
/// now — picking among older ones is a reasonable future addition, not
/// needed for a first version.
pub async fn get_latest_backup_json(
    pool: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> Option<String> {
    let row =
        sqlx::query("SELECT backup_json FROM backups WHERE guild_id = ? ORDER BY id DESC LIMIT 1")
            .bind(guild_id.to_string())
            .fetch_optional(pool)
            .await
            .expect("failed to look up the latest backup")?;

    Some(
        row.try_get("backup_json")
            .expect("backups.backup_json column missing or the wrong type"),
    )
}
