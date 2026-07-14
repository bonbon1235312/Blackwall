use std::sync::Arc;
use std::time::{Duration, Instant};

use twilight_gateway::{
    Config as GatewayConfig, Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _,
};
use twilight_model::guild::audit_log::AuditLogEventType;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

use crate::actions;
use crate::actions::lockdown;
use crate::discord::{embeds, interactions, prefix};
use crate::moderation::invite;
use crate::moderation::nuke::NukeViolation;
use crate::moderation::permissions;
use crate::moderation::raid::{self, RaidViolation};
use crate::moderation::scam;
use crate::moderation::spam::{self, SpamViolation};
use crate::state::AppState;
use crate::storage::models;

/// Connects to Discord's gateway and processes events forever.
///
/// This never returns under normal operation — it's the bot's main loop.
pub async fn run(token: String, state: Arc<AppState>) {
    // Intents tell Discord which events we want to receive. Asking for
    // fewer intents means less data (and fewer privacy concerns), so we
    // only request what the current stage actually uses:
    //   - GUILDS: needed for basic guild info and to keep caches consistent
    //   - GUILD_MESSAGES: needed to see messages for anti-spam/anti-scam
    //   - MESSAGE_CONTENT: needed to read message *text* (this is a
    //     "privileged" intent — it must also be turned on for the bot in
    //     the Discord Developer Portal, under Bot -> Privileged Gateway
    //     Intents, or Discord will refuse the connection)
    //   - GUILD_MEMBERS: needed to receive join events for anti-raid
    //     detection (also privileged — same Developer Portal toggle,
    //     under "Server Members Intent" — required or the connection is
    //     refused, same as MESSAGE_CONTENT above)
    //   - GUILD_MODERATION: needed to receive audit-log entries for
    //     anti-nuke detection. Not privileged (no Developer Portal toggle
    //     needed), but the bot still needs the View Audit Log permission
    //     in the server itself, or Discord simply won't send the events.
    let intents = Intents::GUILDS
        | Intents::GUILD_MESSAGES
        | Intents::MESSAGE_CONTENT
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_MODERATION;

    let config = GatewayConfig::new(token, intents);

    // A single shard is one websocket connection to Discord. Small/medium
    // bots only ever need one; Discord requires more shards once a bot
    // joins enough servers, which is a later-stage concern.
    let mut shard = Shard::with_config(ShardId::ONE, config);

    // Exactly the event types `handle_event`'s match arms below act on.
    // twilight-gateway skips full JSON deserialization for anything
    // outside this mask (it's a dispatch-level filter, not a post-parse
    // discard) — every event type not listed here (typing indicators,
    // presence updates, etc.) is dropped before Blackwall pays to decode
    // it, not after. Doesn't affect Shard-level control frames (heartbeat
    // ack, reconnect, invalid session) — those aren't Dispatch events and
    // are handled by Shard regardless of this mask.
    let wanted_events = EventTypeFlags::READY
        | EventTypeFlags::MESSAGE_CREATE
        | EventTypeFlags::MEMBER_ADD
        | EventTypeFlags::GUILD_AUDIT_LOG_ENTRY_CREATE
        | EventTypeFlags::INTERACTION_CREATE;

    tracing::info!("connecting to the Discord gateway...");

    // `next_event` yields `None` only when the shard is shutting down for
    // good, so this loop is effectively the bot's lifetime.
    while let Some(item) = shard.next_event(wanted_events).await {
        let event = match item {
            Ok(event) => event,
            Err(source) => {
                tracing::warn!(?source, "error receiving gateway event");
                continue;
            }
        };

        // Spawn each event onto its own task so a slow handler (e.g. one
        // waiting on Discord's REST API) can't delay other events.
        tokio::spawn(handle_event(event, Arc::clone(&state)));
    }
}

async fn handle_event(event: Event, state: Arc<AppState>) {
    match event {
        Event::Ready(ready) => {
            tracing::info!(username = %ready.user.name, "Blackwall is online");
        }
        Event::MessageCreate(message) => {
            // Ignore messages from bots (including ourselves) so we never
            // react to our own logs or another bot's output.
            if message.author.bot {
                return;
            }

            tracing::info!(
                author = %message.author.name,
                channel_id = %message.channel_id,
                content = %message.content,
                "message received"
            );

            // Blackwall only moderates servers, not DMs — and per-guild
            // settings (below) need a guild to look up in the first place.
            let Some(guild_id) = message.guild_id else {
                return;
            };

            // `!`/`?` prefix commands are a separate concern from message
            // moderation below — a recognized command message is never
            // itself scanned for scam patterns or counted toward spam.
            if prefix::try_handle(&message, &state).await {
                return;
            }

            let settings = state.settings_cache.get(&state.db, guild_id).await;

            if settings.anti_scam_enabled {
                if let Some(scam_match) = scam::check(&state.scam_matcher, &message.content) {
                    handle_scam_message(&state, &message, guild_id, &scam_match).await;
                    return;
                }
            }

            // Cheap in-memory check first (no invite link in most
            // messages), so the trusted-role DB lookup below only ever
            // runs on an actual candidate match, not on every message.
            if settings.anti_invite_enabled && invite::is_invite_link(&message.content) {
                let is_trusted = models::get_trusted_role_id(&state.db, guild_id)
                    .await
                    .is_some_and(|trusted_role_id| {
                        message
                            .member
                            .as_ref()
                            .is_some_and(|member| member.roles.contains(&trusted_role_id))
                    });

                if !is_trusted {
                    handle_invite_link(&state, &message, guild_id).await;
                    return;
                }
            }

            if settings.anti_spam_enabled {
                let mention_count = message.mentions.len();
                let spam_thresholds = spam::SpamThresholds {
                    burst_threshold: settings.spam_burst_threshold as usize,
                    repeat_threshold: settings.spam_repeat_threshold as usize,
                    mention_threshold: settings.spam_mention_threshold as usize,
                    timeout: Duration::from_secs(settings.spam_timeout_minutes as u64 * 60),
                };
                let violation = state.spam_tracker.check(
                    guild_id,
                    message.author.id,
                    &message.content,
                    mention_count,
                    spam_thresholds,
                );

                if let Some(violation) = violation {
                    handle_spam_violation(
                        &state,
                        &message,
                        guild_id,
                        &violation,
                        spam_thresholds.timeout,
                    )
                    .await;
                }
            }
        }
        Event::MemberAdd(member_add) => {
            let guild_id = member_add.guild_id;

            let settings = state.settings_cache.get(&state.db, guild_id).await;
            if !settings.anti_raid_enabled {
                return;
            }

            let raid_thresholds = raid::RaidThresholds {
                burst_threshold: settings.raid_burst_threshold as usize,
                suspicious_threshold: settings.raid_suspicious_threshold as usize,
                timeout: Duration::from_secs(settings.raid_timeout_minutes as u64 * 60),
            };

            let detector_started = tracing::enabled!(tracing::Level::DEBUG).then(Instant::now);
            let raid_check =
                state
                    .join_tracker
                    .record(guild_id, &member_add.member, raid_thresholds);
            if let Some(started) = detector_started {
                tracing::debug!(
                    detector_ns = started.elapsed().as_nanos(),
                    raid_active = raid_check.raid_active,
                    triggered = raid_check.incident.is_some(),
                    "anti-raid detector completed"
                );
            }

            if let Some(incident) = raid_check.incident {
                let lockdown_started = handle_raid_violation(
                    &state,
                    guild_id,
                    &incident.violation,
                    &incident.window,
                    raid_thresholds.timeout,
                )
                .await;
                if !lockdown_started {
                    state.join_tracker.schedule_incident_retry(guild_id);
                }
            } else if raid_check.raid_active {
                if raid_check.latest.is_suspicious {
                    handle_active_raid_join(
                        &state,
                        guild_id,
                        &raid_check.latest,
                        raid_thresholds.timeout,
                    )
                    .await;
                }
            } else if raid_check.latest.is_suspicious {
                if let Some(seconds_since_removal) =
                    state.evasion_tracker.seconds_since_removal(guild_id)
                {
                    // Deliberately `else if` on the raid branch: a join that's
                    // already part of a full raid response gets a raid embed
                    // with its own timeline — this covers the different case
                    // of a single suspicious rejoin that's too small on its
                    // own to trip the raid thresholds.
                    handle_possible_evasion(
                        &state,
                        guild_id,
                        &raid_check.latest,
                        seconds_since_removal,
                    )
                    .await;
                }
            }
        }
        Event::GuildAuditLogEntryCreate(entry) => {
            // Both fields are documented as always present in this exact
            // event (as opposed to a plain audit-log REST fetch, where
            // they can be absent) — but never trust that blindly.
            let (Some(guild_id), Some(actor_id)) = (entry.guild_id, entry.user_id) else {
                return;
            };

            // Recorded regardless of the anti-nuke toggle below and
            // regardless of whether this single ban/kick trips the nuke
            // threshold — a lone, legitimate staff ban is exactly the
            // case the evasion tracker cares about, not just nuke-scale
            // bursts.
            if matches!(
                entry.action_type,
                AuditLogEventType::MemberBanAdd | AuditLogEventType::MemberKick
            ) {
                state.evasion_tracker.mark_removal(guild_id);
            }

            let settings = state.settings_cache.get(&state.db, guild_id).await;
            if !settings.anti_nuke_enabled {
                return;
            }

            // Independent of the burst counting below: a bot/app added by
            // someone who doesn't hold the guild's trusted role gets
            // reacted to immediately, not only after 3 dangerous actions
            // in 30 seconds. BotAdd still also counts toward that burst
            // either way — this doesn't replace it.
            if entry.action_type == AuditLogEventType::BotAdd {
                handle_unauthorized_bot_add(&state, guild_id, actor_id, entry.target_id).await;
            }

            if let Some(violation) = state.nuke_tracker.record(
                guild_id,
                actor_id,
                entry.action_type,
                settings.nuke_burst_threshold as usize,
            ) {
                handle_nuke_violation(&state, guild_id, actor_id, &violation).await;
            }
        }
        Event::InteractionCreate(interaction) => {
            interactions::handle(interaction.0, &state).await;
        }
        _ => {}
    }
}

/// Deletes a message that matched a scam pattern and, if a log channel is
/// configured, sends staff an embed describing what was removed and why.
async fn handle_scam_message(
    state: &AppState,
    message: &twilight_model::channel::Message,
    guild_id: Id<GuildMarker>,
    scam_match: &scam::ScamMatch,
) {
    tracing::warn!(
        author = %message.author.name,
        channel_id = %message.channel_id,
        category = scam_match.category,
        "deleting message matched by scam filter"
    );

    if let Err(source) = state
        .http
        .delete_message(message.channel_id, message.id)
        .await
    {
        tracing::error!(?source, "failed to delete scam message");
    }

    let description = format!(
        "Deleted a scam/phishing message in channel {} matched as {}.",
        message.channel_id, scam_match.category
    );
    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(message.author.id),
        "scam_message_deleted",
        "high",
        &description,
    )
    .await
    {
        tracing::error!(?source, "failed to record scam security event");
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::scam_message_deleted(message, scam_match);

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, "failed to send scam log embed");
    }
}

/// Deletes a message containing a Discord invite link from someone who
/// doesn't hold this server's trusted role (checked by the caller). No
/// timeout in this pass — deletion only, to avoid punishing a false
/// positive (e.g. someone who should have the trusted role but doesn't
/// yet) more harshly than necessary.
async fn handle_invite_link(state: &AppState, message: &twilight_model::channel::Message, guild_id: Id<GuildMarker>) {
    tracing::warn!(
        author = %message.author.name,
        channel_id = %message.channel_id,
        "deleting message with an invite link"
    );

    if let Err(source) = state
        .http
        .delete_message(message.channel_id, message.id)
        .await
    {
        tracing::error!(?source, "failed to delete invite-link message");
    }

    let description = format!(
        "Deleted an invite-link message in channel {} from a user without the trusted role.",
        message.channel_id
    );
    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(message.author.id),
        "invite_link_deleted",
        "medium",
        &description,
    )
    .await
    {
        tracing::error!(?source, "failed to record invite-link security event");
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::invite_link_deleted(message);

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, "failed to send invite-link log embed");
    }
}

/// Deletes a message that tripped the anti-spam filter, times its author
/// out (unless they're the server owner — see below), and — if a log
/// channel is configured — sends staff an embed explaining what happened.
async fn handle_spam_violation(
    state: &AppState,
    message: &twilight_model::channel::Message,
    guild_id: Id<GuildMarker>,
    violation: &SpamViolation,
    timeout_duration: Duration,
) {
    tracing::warn!(
        author = %message.author.name,
        channel_id = %message.channel_id,
        reason = %violation.description(),
        "actioning spam violation"
    );

    if let Err(source) = state
        .http
        .delete_message(message.channel_id, message.id)
        .await
    {
        tracing::error!(?source, "failed to delete spam message");
    }

    // Discord never allows moderating the guild owner — not a permissions
    // gap, a hard rule baked into the API. Checking this first (a cheap
    // local DB lookup) avoids a guaranteed-to-fail timeout call and the
    // misleading "Missing Permissions" error it would otherwise log,
    // which reads like a fixable bot-permission problem when it isn't.
    let is_owner = models::get_owner_id(&state.db, guild_id).await == Some(message.author.id);

    let timed_out = if is_owner {
        tracing::info!(
            author = %message.author.name,
            channel_id = %message.channel_id,
            "skipping timeout: author is the server owner, which Discord never allows moderating"
        );
        false
    } else {
        match state
            .http
            .update_guild_member(guild_id, message.author.id)
            .communication_disabled_until(Some(spam::timeout_until(timeout_duration)))
            .await
        {
            Ok(_) => true,
            Err(source) => {
                tracing::error!(?source, "failed to time out spamming user");
                false
            }
        }
    };

    let description = if timed_out {
        format!(
            "Deleted a spam message in channel {} and timed out the user: {}.",
            message.channel_id,
            violation.description()
        )
    } else if is_owner {
        format!(
            "Deleted a spam message in channel {} from the server owner: {}. Timeout skipped \
            — Discord does not allow moderating the owner.",
            message.channel_id,
            violation.description()
        )
    } else {
        format!(
            "Deleted a spam message in channel {}, but could not time out the user: {}.",
            message.channel_id,
            violation.description()
        )
    };
    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(message.author.id),
        "spam_timeout",
        "medium",
        &description,
    )
    .await
    {
        tracing::error!(?source, "failed to record spam security event");
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::spam_timeout(message, violation, timed_out, is_owner);

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, "failed to send spam log embed");
    }
}

/// Responds to a detected raid: locks every text channel (reusing the
/// same `actions::lockdown::engage` that `/lockdown` calls manually),
/// times out the individually-suspicious joiners from the triggering
/// window, and alerts staff with a full timeline.
async fn handle_raid_violation(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    violation: &RaidViolation,
    window: &[raid::JoinRecord],
    raid_timeout: Duration,
) -> bool {
    tracing::warn!(
        %guild_id,
        reason = %violation.description(),
        "raid detected, engaging lockdown"
    );

    let lockdown_report = match lockdown::engage(&state.http, &state.db, guild_id).await {
        Ok(report) => Some(report),
        Err(source) => {
            tracing::error!(?source, %guild_id, "failed to engage lockdown during raid response");
            None
        }
    };
    let lockdown_started = lockdown_report.is_some();

    // Only the individually-suspicious joiners get timed out — the burst
    // threshold alone can trip on a legitimate growth spurt for a small
    // server, so blanket-timing-out every joiner in the window would
    // punish people who did nothing suspicious themselves.
    let owner_id = models::get_owner_id(&state.db, guild_id).await;
    let mut timed_out_count = 0;

    for record in window.iter().filter(|record| record.is_suspicious) {
        if timeout_raid_joiner(state, guild_id, record, owner_id, raid_timeout).await {
            timed_out_count += 1;
        }
    }

    let channels_locked = lockdown_report
        .as_ref()
        .map_or(0, |report| report.channels_locked);
    let description = format!(
        "Raid detected: {}. Locked {channels_locked} channel(s), timed out {timed_out_count} \
        suspicious joiner(s).",
        violation.description()
    );

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        None,
        "raid_detected",
        "critical",
        &description,
    )
    .await
    {
        tracing::error!(?source, %guild_id, "failed to record raid security event");
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return lockdown_started;
    };

    let embed = embeds::raid_detected(violation, window, timed_out_count);

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, %guild_id, "failed to send raid log embed");
    }

    lockdown_started
}

/// Applies the raid timeout to a suspicious account that joined after the
/// current raid incident was already raised. This keeps containment active
/// without repeating the full lockdown, database event, and staff embed.
async fn handle_active_raid_join(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    record: &raid::JoinRecord,
    raid_timeout: Duration,
) {
    let owner_id = models::get_owner_id(&state.db, guild_id).await;
    if timeout_raid_joiner(state, guild_id, record, owner_id, raid_timeout).await {
        tracing::info!(
            %guild_id,
            user_id = %record.user_id,
            "timed out suspicious joiner during active raid"
        );
    }
}

async fn timeout_raid_joiner(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    record: &raid::JoinRecord,
    owner_id: Option<Id<UserMarker>>,
    raid_timeout: Duration,
) -> bool {
    // Same owner-immunity check as anti-spam: Discord never allows
    // moderating the guild owner, regardless of permissions.
    if owner_id == Some(record.user_id) {
        return false;
    }

    match state
        .http
        .update_guild_member(guild_id, record.user_id)
        .communication_disabled_until(Some(raid::raid_timeout_until(raid_timeout)))
        .await
    {
        Ok(_) => true,
        Err(source) => {
            tracing::error!(
                ?source,
                %guild_id,
                user_id = %record.user_id,
                "failed to time out suspicious joiner"
            );
            false
        }
    }
}

/// Flags (never blocks) a single suspicious join that happened shortly
/// after a ban or kick in this server. This is a timing correlation, not
/// proof of anything — Blackwall doesn't collect IP addresses or device
/// fingerprints to actually link the new account to the removed one, so
/// the log embed says exactly that and leaves the call to staff.
async fn handle_possible_evasion(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    record: &raid::JoinRecord,
    seconds_since_removal: u64,
) {
    tracing::info!(
        %guild_id,
        user_id = %record.user_id,
        seconds_since_removal,
        "suspicious join shortly after a ban/kick"
    );

    let description = format!(
        "<@{}> ({}) joined {} after a ban or kick in this server, and looked individually \
        suspicious. This is a timing correlation only, not confirmation it's the same person.",
        record.user_id,
        record.username,
        format_duration_ago(seconds_since_removal)
    );

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(record.user_id),
        "possible_evasion",
        "medium",
        &description,
    )
    .await
    {
        tracing::error!(?source, %guild_id, "failed to record possible-evasion security event");
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::possible_evasion(record, seconds_since_removal);

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, %guild_id, "failed to send possible-evasion log embed");
    }
}

/// Renders a seconds count as "3 minute(s) ago" / "1 hour(s) ago" for the
/// possible-evasion description — precise-to-the-second would read as
/// falsely exact for something that's already an approximation.
fn format_duration_ago(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds} second(s) ago")
    } else if seconds < 60 * 60 {
        format!("{} minute(s) ago", seconds / 60)
    } else {
        format!("{} hour(s) ago", seconds / (60 * 60))
    }
}

/// Responds to a suspected nuke attempt: strips the actor's dangerous
/// roles, quarantines them, locks every text channel (reusing
/// `actions::lockdown::engage`, same as the raid response), and alerts
/// the owner both in the log channel *and* by DM — the log channel may
/// have just been one of the things the attacker deleted.
///
/// Never actions the guild owner: Discord wouldn't let a role-removal or
/// quarantine stick against them in the ways that matter anyway, and if
/// the actual owner account is doing this, it's not something Blackwall
/// can distinguish from legitimate (if unusual) admin work.
async fn handle_nuke_violation(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    actor_id: Id<UserMarker>,
    violation: &NukeViolation,
) {
    tracing::error!(
        %guild_id,
        %actor_id,
        reason = %violation.description(),
        "nuke attempt detected"
    );

    let owner_id = models::get_owner_id(&state.db, guild_id).await;

    if owner_id == Some(actor_id) {
        tracing::warn!(
            %guild_id,
            %actor_id,
            "dangerous activity from the server owner — Blackwall does not action the owner"
        );
        return;
    }

    let roles_removed = strip_dangerous_roles(state, guild_id, actor_id).await;

    let quarantined = if let Some(quarantine_role_id) =
        models::get_quarantine_role_id(&state.db, guild_id).await
    {
        state
            .http
            .add_guild_member_role(guild_id, actor_id, quarantine_role_id)
            .await
            .is_ok()
    } else {
        false
    };

    let lockdown_report = match lockdown::engage(&state.http, &state.db, guild_id).await {
        Ok(report) => Some(report),
        Err(source) => {
            tracing::error!(?source, %guild_id, "failed to engage lockdown during nuke response");
            None
        }
    };
    let channels_locked = lockdown_report.map_or(0, |report| report.channels_locked);

    let description = format!(
        "Nuke attempt detected: {}. Stripped {roles_removed} dangerous role(s) from <@{actor_id}>, \
        quarantined: {quarantined}, locked {channels_locked} channel(s).",
        violation.description()
    );

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(actor_id),
        "nuke_detected",
        "critical",
        &description,
    )
    .await
    {
        tracing::error!(?source, %guild_id, "failed to record nuke security event");
    }

    // The owner is alerted by DM specifically because the log channel may
    // have just been deleted by the same actor this response is about.
    if let Some(owner_id) = owner_id {
        let dm_content = format!(
            "**Blackwall security alert**\n{description}\n\nThis happened in a server you own. \
            Check the server's audit log to confirm what was affected."
        );

        if let Ok(dm_channel_response) = state.http.create_private_channel(owner_id).await {
            if let Ok(dm_channel) = dm_channel_response.model().await {
                if let Err(source) = state
                    .http
                    .create_message(dm_channel.id)
                    .content(&dm_content)
                    .await
                {
                    tracing::error!(?source, %guild_id, %owner_id, "failed to DM owner about nuke attempt");
                }
            }
        }
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::nuke_detected(
        violation,
        actor_id,
        roles_removed,
        quarantined,
        channels_locked,
    );

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, %guild_id, "failed to send nuke log embed");
    }
}

/// Same reasoning as `actions::lockdown::LOCK_CONCURRENCY` — bounds how many
/// role-removal REST calls run at once instead of one at a time.
const ROLE_STRIP_CONCURRENCY: usize = 10;

/// Removes every role `actor_id` holds that `permissions::dangerous_role_ids`
/// flags, returning how many were actually removed. Shared by the nuke
/// response above and the unauthorized-bot-add gate below — both treat
/// "strip an actor's admin-level roles" as the same corrective action.
async fn strip_dangerous_roles(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    actor_id: Id<UserMarker>,
) -> usize {
    let Ok(roles_response) = state.http.roles(guild_id).await else {
        return 0;
    };
    let Ok(roles) = roles_response.model().await else {
        return 0;
    };
    let dangerous_ids = permissions::dangerous_role_ids(&roles);

    let Ok(member_response) = state.http.guild_member(guild_id, actor_id).await else {
        return 0;
    };
    let Ok(member) = member_response.model().await else {
        return 0;
    };

    let actions: Vec<actions::concurrent::BoxedAction> = member
        .roles
        .iter()
        .filter(|role_id| dangerous_ids.contains(role_id))
        .map(|role_id| {
            let role_id = *role_id;
            Box::pin(async move {
                match state
                    .http
                    .remove_guild_member_role(guild_id, actor_id, role_id)
                    .await
                {
                    Ok(_) => true,
                    Err(source) => {
                        tracing::error!(?source, %guild_id, %actor_id, %role_id, "failed to strip dangerous role");
                        false
                    }
                }
            }) as actions::concurrent::BoxedAction
        })
        .collect();

    let (roles_removed, _) = actions::concurrent::dispatch(actions, ROLE_STRIP_CONCURRENCY).await;

    roles_removed
}

/// Reacts when a bot/app is added to the server by someone who doesn't
/// hold the guild's configured trusted role: kicks the newly-added bot and
/// strips the adding actor's dangerous roles, the same severity as a
/// confirmed nuke attempt. A guild with no trusted role configured yet has
/// no one to exempt, so this is a no-op there — BotAdd still counts toward
/// the generic nuke-burst threshold either way (see the caller).
async fn handle_unauthorized_bot_add(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    actor_id: Id<UserMarker>,
    target_id: Option<Id<twilight_model::id::marker::GenericMarker>>,
) {
    let Some(trusted_role_id) = models::get_trusted_role_id(&state.db, guild_id).await else {
        return;
    };

    let owner_id = models::get_owner_id(&state.db, guild_id).await;
    if owner_id == Some(actor_id) {
        return;
    }

    let has_trusted_role = match state.http.guild_member(guild_id, actor_id).await {
        Ok(response) => match response.model().await {
            Ok(member) => member.roles.contains(&trusted_role_id),
            Err(source) => {
                tracing::error!(?source, %guild_id, %actor_id, "failed to decode member while checking the trusted role");
                return;
            }
        },
        Err(source) => {
            tracing::error!(?source, %guild_id, %actor_id, "failed to look up member while checking the trusted role");
            return;
        }
    };

    if has_trusted_role {
        return;
    }

    let bot_kicked = match target_id {
        Some(target_id) => {
            let bot_user_id = target_id.cast::<UserMarker>();
            match state.http.remove_guild_member(guild_id, bot_user_id).await {
                Ok(_) => true,
                Err(source) => {
                    tracing::error!(?source, %guild_id, %bot_user_id, "failed to kick unauthorized bot");
                    false
                }
            }
        }
        None => {
            tracing::warn!(%guild_id, "BotAdd audit-log entry had no target_id, could not kick the bot");
            false
        }
    };

    let roles_removed = strip_dangerous_roles(state, guild_id, actor_id).await;

    tracing::warn!(
        %guild_id,
        %actor_id,
        bot_kicked,
        roles_removed,
        "unauthorized bot add: actor did not hold the trusted role"
    );

    let description = format!(
        "<@{actor_id}> added a bot/app without holding this server's trusted role. Bot removed: \
        {}. Stripped {roles_removed} dangerous role(s) from <@{actor_id}>.",
        if bot_kicked { "yes" } else { "no — check the bot's Kick Members permission" }
    );

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(actor_id),
        "unauthorized_bot_add",
        "critical",
        &description,
    )
    .await
    {
        tracing::error!(?source, %guild_id, "failed to record unauthorized-bot-add security event");
    }

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::unauthorized_bot_add(actor_id, bot_kicked, roles_removed);

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, %guild_id, "failed to send unauthorized-bot-add log embed");
    }
}
