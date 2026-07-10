use sqlx::SqlitePool;
use twilight_http::Client as HttpClient;
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::http::permission_overwrite::{
    PermissionOverwrite as HttpPermissionOverwrite, PermissionOverwriteType as HttpOverwriteType,
};
use twilight_model::id::{Id, marker::GuildMarker};

use crate::storage::models;

/// Summary of a `/lockdown` (or automatic raid-response lockdown) run.
pub struct LockdownReport {
    pub channels_locked: usize,
    pub channels_failed: usize,
}

/// Summary of an `/unlockdown` run.
pub struct UnlockdownReport {
    pub channels_restored: usize,
    pub channels_failed: usize,
}

/// Locks every text channel in a guild: denies `SEND_MESSAGES` for
/// `@everyone`. Before changing anything, snapshots whatever `@everyone`
/// overwrite each channel already had (or the fact that it had none) so
/// [`revert`] can restore the exact prior state, not just delete
/// whatever this function added — a channel that already denied
/// `SEND_MESSAGES` to `@everyone` before lockdown must still deny it
/// afterward, not end up with no overwrite at all.
///
/// Called from both `/lockdown` and the automatic anti-raid response —
/// this is the whole reason `actions::lockdown` exists as its own module
/// instead of being inlined at either call site.
pub async fn engage(
    http: &HttpClient,
    db: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> Result<LockdownReport, twilight_http::Error> {
    let channels = http
        .guild_channels(guild_id)
        .await?
        .model()
        .await
        .expect("Discord sent back channels we couldn't read");

    // The @everyone role's ID is always identical to the guild's ID.
    let everyone_role_id: Id<twilight_model::id::marker::RoleMarker> = Id::new(guild_id.get());

    let mut channels_locked = 0;
    let mut channels_failed = 0;

    for channel in channels
        .iter()
        .filter(|channel| channel.kind == ChannelType::GuildText)
    {
        let existing = channel
            .permission_overwrites
            .as_ref()
            .and_then(|overwrites| {
                overwrites
                    .iter()
                    .find(|overwrite| overwrite.id.get() == guild_id.get())
            });

        models::save_lockdown_snapshot(
            db,
            guild_id,
            channel.id,
            existing.map(|overwrite| (overwrite.allow, overwrite.deny)),
        )
        .await;

        let mut new_allow = existing.map_or(Permissions::empty(), |overwrite| overwrite.allow);
        new_allow.remove(Permissions::SEND_MESSAGES);

        let mut new_deny = existing.map_or(Permissions::empty(), |overwrite| overwrite.deny);
        new_deny.insert(Permissions::SEND_MESSAGES);

        let overwrite = HttpPermissionOverwrite {
            allow: Some(new_allow),
            deny: Some(new_deny),
            id: everyone_role_id.cast(),
            kind: HttpOverwriteType::Role,
        };

        match http.update_channel_permission(channel.id, &overwrite).await {
            Ok(_) => channels_locked += 1,
            Err(source) => {
                tracing::error!(?source, channel_id = %channel.id, "failed to lock channel");
                channels_failed += 1;
            }
        }
    }

    models::set_lockdown_enabled(db, guild_id, true).await;

    Ok(LockdownReport {
        channels_locked,
        channels_failed,
    })
}

/// Restores every channel's `@everyone` overwrite to exactly what
/// [`engage`] snapshotted, then forgets the snapshot. Safe to call on a
/// guild that isn't currently locked down — it just finds no snapshots
/// and restores nothing.
pub async fn revert(
    http: &HttpClient,
    db: &SqlitePool,
    guild_id: Id<GuildMarker>,
) -> UnlockdownReport {
    let snapshots = models::take_lockdown_snapshots(db, guild_id).await;
    let everyone_role_id: Id<twilight_model::id::marker::RoleMarker> = Id::new(guild_id.get());

    let mut channels_restored = 0;
    let mut channels_failed = 0;

    for snapshot in snapshots {
        let success = if snapshot.had_overwrite {
            let overwrite = HttpPermissionOverwrite {
                allow: Some(snapshot.everyone_allow),
                deny: Some(snapshot.everyone_deny),
                id: everyone_role_id.cast(),
                kind: HttpOverwriteType::Role,
            };

            http.update_channel_permission(snapshot.channel_id, &overwrite)
                .await
                .inspect_err(|source| {
                    tracing::error!(?source, channel_id = %snapshot.channel_id, "failed to restore channel overwrite");
                })
                .is_ok()
        } else {
            http.delete_channel_permission(snapshot.channel_id)
                .role(everyone_role_id)
                .await
                .inspect_err(|source| {
                    tracing::error!(?source, channel_id = %snapshot.channel_id, "failed to remove lockdown overwrite");
                })
                .is_ok()
        };

        if success {
            channels_restored += 1;
        } else {
            channels_failed += 1;
        }
    }

    models::set_lockdown_enabled(db, guild_id, false).await;

    UnlockdownReport {
        channels_restored,
        channels_failed,
    }
}
