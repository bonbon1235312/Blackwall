use sqlx::{PgPool, Row};
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
    /// Exempts a member from the bot-add gate (`gateway`'s `BotAdd`
    /// handling) and invite-link deletion (`moderation::invite`). `None`
    /// means neither feature has anyone to exempt yet, not that they're
    /// disabled — see each feature's own check.
    pub trusted_role_id: Option<Id<RoleMarker>>,
}

/// Creates or updates a guild's configuration, and makes sure a
/// `guild_settings` row exists for it with sensible defaults.
///
/// Existing feature toggles in `guild_settings` are never touched by this:
/// it only fills them in the first time, so changing a channel or role in
/// the setup panel cannot silently undo a setting an admin already changed.
pub async fn upsert_guild_config(pool: &PgPool, config: &GuildConfig) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO guilds (guild_id, owner_id, log_channel_id, verified_role_id, quarantine_role_id, trusted_role_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (guild_id) DO UPDATE SET
            owner_id = excluded.owner_id,
            log_channel_id = excluded.log_channel_id,
            verified_role_id = excluded.verified_role_id,
            quarantine_role_id = excluded.quarantine_role_id,
            trusted_role_id = excluded.trusted_role_id",
    )
    .bind(config.guild_id.to_string())
    .bind(config.owner_id.to_string())
    .bind(config.log_channel_id.map(|id| id.to_string()))
    .bind(config.verified_role_id.map(|id| id.to_string()))
    .bind(config.quarantine_role_id.map(|id| id.to_string()))
    .bind(config.trusted_role_id.map(|id| id.to_string()))
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO guild_settings (guild_id) VALUES ($1) ON CONFLICT (guild_id) DO NOTHING",
    )
    .bind(config.guild_id.to_string())
    .execute(pool)
    .await?;

    Ok(())
}

/// Looks up a guild's saved setup choices, if it has been initialized.
pub async fn get_guild_config(pool: &PgPool, guild_id: Id<GuildMarker>) -> Option<GuildConfig> {
    let row = sqlx::query(
        "SELECT owner_id, log_channel_id, verified_role_id, quarantine_role_id, trusted_role_id
         FROM guilds WHERE guild_id = $1",
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
    let trusted_role_id: Option<String> = row
        .try_get("trusted_role_id")
        .expect("guilds.trusted_role_id column missing or the wrong type");

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
        trusted_role_id: trusted_role_id.map(|raw| {
            raw.parse()
                .expect("trusted_role_id stored in the database was not a valid Discord ID")
        }),
    })
}

/// Updates the log channel selected in the setup panel.
pub async fn set_log_channel_id(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE guilds SET log_channel_id = $1 WHERE guild_id = $2")
        .bind(channel_id.to_string())
        .bind(guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Updates the role granted after a successful OAuth verification.
pub async fn set_verified_role_id(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE guilds SET verified_role_id = $1 WHERE guild_id = $2")
        .bind(role_id.to_string())
        .bind(guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Updates the role used to quarantine suspicious members.
pub async fn set_quarantine_role_id(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE guilds SET quarantine_role_id = $1 WHERE guild_id = $2")
        .bind(role_id.to_string())
        .bind(guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Updates the role that exempts a member from the bot-add gate and
/// invite-link deletion.
pub async fn set_trusted_role_id(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    role_id: Id<RoleMarker>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE guilds SET trusted_role_id = $1 WHERE guild_id = $2")
        .bind(role_id.to_string())
        .bind(guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Looks up the role configured by `/setup` to exempt a member from the
/// bot-add gate and invite-link deletion. `None` means no trusted role has
/// been configured — both features treat that as "nothing to exempt yet,"
/// not "feature disabled."
pub async fn get_trusted_role_id(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
) -> Option<Id<RoleMarker>> {
    let row = sqlx::query("SELECT trusted_role_id FROM guilds WHERE guild_id = $1")
        .bind(guild_id.to_string())
        .fetch_optional(pool)
        .await
        .expect("failed to look up a guild's trusted role")?;

    let raw: Option<String> = row
        .try_get("trusted_role_id")
        .expect("guilds.trusted_role_id column missing or the wrong type");

    raw.map(|raw| {
        raw.parse()
            .expect("trusted_role_id stored in the database was not a valid Discord ID")
    })
}

/// A guild's feature toggles and configurable detection thresholds,
/// mirroring a row in the `guild_settings` table. Only the fields
/// something actually reads are included here — the rest of the columns
/// get their own field once the feature that uses them exists
/// (verification).
///
/// `Clone` is needed by `storage::cache::SettingsCache`, which hands back
/// an owned copy from its in-memory cache rather than a reference.
#[derive(Clone)]
pub struct GuildSettings {
    pub anti_spam_enabled: bool,
    pub anti_scam_enabled: bool,
    pub anti_raid_enabled: bool,
    pub anti_nuke_enabled: bool,
    pub anti_invite_enabled: bool,
    /// This many joins within the anti-raid window counts as a raid-scale
    /// burst on its own. See `moderation::raid`.
    pub raid_burst_threshold: i32,
    /// This many individually-suspicious joins within the window counts
    /// as a raid even below the burst threshold.
    pub raid_suspicious_threshold: i32,
    /// This many dangerous audit-log actions by the same actor within 30
    /// seconds counts as a nuke attempt. See `moderation::nuke`.
    pub nuke_burst_threshold: i32,
    /// This many messages within the anti-spam window counts as a burst.
    pub spam_burst_threshold: i32,
    /// This many identical messages in a row counts as copy-paste spam.
    pub spam_repeat_threshold: i32,
    /// This many mentions in a single message counts as mention spam.
    pub spam_mention_threshold: i32,
    pub spam_timeout_minutes: i32,
    pub raid_timeout_minutes: i32,
}

/// Looks up a guild's feature toggles.
///
/// A guild that hasn't run `/setup` yet has no `guild_settings` row at
/// all — every toggle is treated as "on" in that case, matching the
/// smart defaults `/setup` itself would have saved.
///
/// This runs on every message, join, and audit-log entry, so callers
/// should almost always go through `storage::cache::SettingsCache`
/// instead of calling this directly — it's still here as the thing the
/// cache falls back to on a miss.
pub async fn get_guild_settings(pool: &PgPool, guild_id: Id<GuildMarker>) -> GuildSettings {
    let row = sqlx::query(
        "SELECT anti_spam_enabled, anti_scam_enabled, anti_raid_enabled, anti_nuke_enabled,
                anti_invite_enabled, raid_burst_threshold, raid_suspicious_threshold,
                nuke_burst_threshold, spam_burst_threshold, spam_repeat_threshold,
                spam_mention_threshold, spam_timeout_minutes, raid_timeout_minutes
         FROM guild_settings WHERE guild_id = $1",
    )
    .bind(guild_id.to_string())
    .fetch_optional(pool)
    .await
    .expect("failed to look up guild settings");

    // Defaults here must match `guild_settings`' own column defaults in
    // schema.sql: a guild with no row yet (hasn't run /setup) gets exactly
    // what a freshly-created row would have.
    let Some(row) = row else {
        return GuildSettings {
            anti_spam_enabled: true,
            anti_scam_enabled: true,
            anti_raid_enabled: true,
            anti_nuke_enabled: true,
            anti_invite_enabled: true,
            raid_burst_threshold: 10,
            raid_suspicious_threshold: 5,
            nuke_burst_threshold: 3,
            spam_burst_threshold: 6,
            spam_repeat_threshold: 3,
            spam_mention_threshold: 5,
            spam_timeout_minutes: 10,
            raid_timeout_minutes: 1440,
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
        anti_invite_enabled: row
            .try_get("anti_invite_enabled")
            .expect("guild_settings.anti_invite_enabled column missing or the wrong type"),
        raid_burst_threshold: row
            .try_get("raid_burst_threshold")
            .expect("guild_settings.raid_burst_threshold column missing or the wrong type"),
        raid_suspicious_threshold: row
            .try_get("raid_suspicious_threshold")
            .expect("guild_settings.raid_suspicious_threshold column missing or the wrong type"),
        nuke_burst_threshold: row
            .try_get("nuke_burst_threshold")
            .expect("guild_settings.nuke_burst_threshold column missing or the wrong type"),
        spam_burst_threshold: row
            .try_get("spam_burst_threshold")
            .expect("guild_settings.spam_burst_threshold column missing or the wrong type"),
        spam_repeat_threshold: row
            .try_get("spam_repeat_threshold")
            .expect("guild_settings.spam_repeat_threshold column missing or the wrong type"),
        spam_mention_threshold: row
            .try_get("spam_mention_threshold")
            .expect("guild_settings.spam_mention_threshold column missing or the wrong type"),
        spam_timeout_minutes: row
            .try_get("spam_timeout_minutes")
            .expect("guild_settings.spam_timeout_minutes column missing or the wrong type"),
        raid_timeout_minutes: row
            .try_get("raid_timeout_minutes")
            .expect("guild_settings.raid_timeout_minutes column missing or the wrong type"),
    }
}

/// Updates one of a guild's configurable detection thresholds. Used by
/// `/config` — `field` must be a literal column name from `guild_settings`
/// (never user input) since it's interpolated into the query rather than
/// bound, which sqlx's query builder doesn't support for column names.
async fn set_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    field: &'static str,
    value: i32,
) -> Result<(), sqlx::Error> {
    let query = format!("UPDATE guild_settings SET {field} = $1 WHERE guild_id = $2");
    sqlx::query(&query)
        .bind(value)
        .bind(guild_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn set_raid_burst_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "raid_burst_threshold", value).await
}

pub async fn set_raid_suspicious_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "raid_suspicious_threshold", value).await
}

pub async fn set_nuke_burst_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "nuke_burst_threshold", value).await
}

pub async fn set_spam_burst_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "spam_burst_threshold", value).await
}

pub async fn set_spam_repeat_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "spam_repeat_threshold", value).await
}

pub async fn set_spam_mention_threshold(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "spam_mention_threshold", value).await
}

pub async fn set_spam_timeout_minutes(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "spam_timeout_minutes", value).await
}

pub async fn set_raid_timeout_minutes(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    value: i32,
) -> Result<(), sqlx::Error> {
    set_threshold(pool, guild_id, "raid_timeout_minutes", value).await
}

/// Looks up where a guild's moderation logs should be sent, if configured.
pub async fn get_log_channel_id(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
) -> Option<Id<ChannelMarker>> {
    let row = sqlx::query("SELECT log_channel_id FROM guilds WHERE guild_id = $1")
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
pub async fn get_owner_id(pool: &PgPool, guild_id: Id<GuildMarker>) -> Option<Id<UserMarker>> {
    let row = sqlx::query("SELECT owner_id FROM guilds WHERE guild_id = $1")
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
pub async fn get_guilds_owned_by(pool: &PgPool, user_id: Id<UserMarker>) -> Vec<Id<GuildMarker>> {
    let rows = sqlx::query("SELECT guild_id FROM guilds WHERE owner_id = $1")
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
) -> Result<Option<Id<RoleMarker>>, sqlx::Error> {
    let Some(row) = sqlx::query("SELECT verified_role_id FROM guilds WHERE guild_id = $1")
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
) -> Option<Id<RoleMarker>> {
    let row = sqlx::query("SELECT quarantine_role_id FROM guilds WHERE guild_id = $1")
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    method: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO verified_users (guild_id, user_id, method) VALUES ($1, $2, $3)
         ON CONFLICT (guild_id, user_id) DO UPDATE SET
            method = excluded.method,
            verified_at = now()",
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    user_id: Option<Id<UserMarker>>,
    event_type: &str,
    severity: &str,
    description: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO security_events (guild_id, user_id, event_type, severity, description)
         VALUES ($1, $2, $3, $4, $5)",
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    limit: i64,
) -> Vec<SecurityEvent> {
    let rows = sqlx::query(
        "SELECT event_type, severity, description, created_at::text AS created_at
         FROM security_events
         WHERE guild_id = $1 ORDER BY id DESC LIMIT $2",
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    existing: Option<(Permissions, Permissions)>,
) {
    let (had_overwrite, allow, deny) = match existing {
        Some((allow, deny)) => (true, Some(allow.bits() as i64), Some(deny.bits() as i64)),
        None => (false, None, None),
    };

    sqlx::query(
        "INSERT INTO lockdown_snapshots
            (guild_id, channel_id, had_overwrite, everyone_allow, everyone_deny)
         VALUES ($1, $2, $3, $4, $5)",
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
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
) -> Vec<LockdownSnapshot> {
    let rows = sqlx::query(
        "SELECT channel_id, had_overwrite, everyone_allow, everyone_deny
         FROM lockdown_snapshots WHERE guild_id = $1",
    )
    .bind(guild_id.to_string())
    .fetch_all(pool)
    .await
    .expect("failed to read lockdown snapshots");

    sqlx::query("DELETE FROM lockdown_snapshots WHERE guild_id = $1")
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

/// Saves a `/backup` snapshot. `backup_json` is a serialized
/// `discord::backup::GuildBackup` — storage doesn't know or care about its
/// shape, just that it's a string to keep and hand back later.
pub async fn create_backup(pool: &PgPool, guild_id: Id<GuildMarker>, backup_json: &str) {
    sqlx::query("INSERT INTO backups (guild_id, backup_json) VALUES ($1, $2::jsonb)")
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
pub async fn get_latest_backup_json(pool: &PgPool, guild_id: Id<GuildMarker>) -> Option<String> {
    let row = sqlx::query(
        "SELECT backup_json::text AS backup_json FROM backups
         WHERE guild_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(guild_id.to_string())
    .fetch_optional(pool)
    .await
    .expect("failed to look up the latest backup")?;

    Some(
        row.try_get("backup_json")
            .expect("backups.backup_json column missing or the wrong type"),
    )
}

/// Overwrites a guild's cached security score — called after `/setup` and
/// `/security-score` recompute `moderation::permissions::PermissionFindings`,
/// so the owner-dashboard website can show a score without ever calling
/// Discord's API itself.
pub async fn upsert_security_score(
    pool: &PgPool,
    guild_id: Id<GuildMarker>,
    score: i64,
    critical_findings: &[String],
    medium_findings: &[String],
) {
    // Bound as plain text + cast to jsonb in the query, same pattern as
    // `create_backup` below — avoids needing sqlx's `json` feature just
    // for this one write.
    let critical_json =
        serde_json::to_string(critical_findings).expect("Vec<String> should always serialize");
    let medium_json =
        serde_json::to_string(medium_findings).expect("Vec<String> should always serialize");

    sqlx::query(
        "INSERT INTO security_scores (guild_id, score, critical_findings, medium_findings, computed_at)
         VALUES ($1, $2, $3::jsonb, $4::jsonb, now())
         ON CONFLICT (guild_id) DO UPDATE SET
            score = excluded.score,
            critical_findings = excluded.critical_findings,
            medium_findings = excluded.medium_findings,
            computed_at = excluded.computed_at",
    )
    .bind(guild_id.to_string())
    .bind(score)
    .bind(critical_json)
    .bind(medium_json)
    .execute(pool)
    .await
    .expect("failed to save security score");
}
