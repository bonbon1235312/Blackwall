use std::time::{SystemTime, UNIX_EPOCH};

use twilight_model::channel::Message;
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use crate::actions::lockdown::{LockdownReport, UnlockdownReport};
use crate::moderation::nuke::NukeViolation;
use crate::moderation::raid::{JoinRecord, RaidViolation};
use crate::moderation::scam::ScamMatch;
use crate::moderation::spam::SpamViolation;
use crate::storage::models::GuildConfig;
use crate::verification::oauth::DiscordUser;

/// Red — used for anything that resulted in a message being removed or a
/// user being actioned.
const COLOR_DANGER: u32 = 0xE0_2B_2B;

/// Green — used for confirmations, like a completed `/setup`.
const COLOR_SUCCESS: u32 = 0x2E_C4_6B;

/// Amber — used for informational events that aren't a problem, but
/// aren't a full success either (e.g. a best-effort support-server join
/// that didn't happen).
const COLOR_NEUTRAL: u32 = 0xE8_A9_3E;

/// Builds the log embed sent when a message is deleted for matching a
/// known scam/phishing pattern.
pub fn scam_message_deleted(message: &Message, scam_match: &ScamMatch) -> Embed {
    EmbedBuilder::new()
        .title("Message deleted — scam pattern detected")
        .color(COLOR_DANGER)
        .field(EmbedFieldBuilder::new("User", format!("<@{}>", message.author.id)).inline())
        .field(EmbedFieldBuilder::new("Channel", format!("<#{}>", message.channel_id)).inline())
        .field(EmbedFieldBuilder::new("Category", scam_match.category).inline())
        .field(EmbedFieldBuilder::new(
            "Matched phrase",
            format!("`{}`", scam_match.phrase),
        ))
        .field(EmbedFieldBuilder::new(
            "Original message",
            truncate(&message.content, 1000),
        ))
        .build()
}

/// Builds the log embed sent when a message is deleted for containing a
/// Discord invite link from a user without the trusted role.
pub fn invite_link_deleted(message: &Message) -> Embed {
    EmbedBuilder::new()
        .title("Message deleted — invite link detected")
        .color(COLOR_DANGER)
        .field(EmbedFieldBuilder::new("User", format!("<@{}>", message.author.id)).inline())
        .field(EmbedFieldBuilder::new("Channel", format!("<#{}>", message.channel_id)).inline())
        .field(EmbedFieldBuilder::new(
            "Original message",
            truncate(&message.content, 1000),
        ))
        .build()
}

/// Builds the log embed sent when a message trips the anti-spam filter.
///
/// `timed_out` and `is_owner` let this be honest about what actually
/// happened: Discord never allows moderating the guild owner (not a
/// permissions gap — a hard API rule), so that case gets its own title
/// and color rather than looking like the timeout silently worked.
pub fn spam_timeout(
    message: &Message,
    violation: &SpamViolation,
    timed_out: bool,
    is_owner: bool,
) -> Embed {
    let (title, color, outcome) = if timed_out {
        (
            "User timed out — spam detected",
            COLOR_DANGER,
            "Message deleted and user timed out for 10 minutes.".to_string(),
        )
    } else if is_owner {
        (
            "Message deleted — spam detected (server owner)",
            COLOR_NEUTRAL,
            "Message deleted. Timeout skipped — Discord does not allow moderating the server owner.".to_string(),
        )
    } else {
        (
            "Message deleted — timeout failed",
            COLOR_DANGER,
            "Message deleted, but Blackwall could not time out the user. Check the bot's \
            Timeout Members permission and its role position."
                .to_string(),
        )
    };

    EmbedBuilder::new()
        .title(title)
        .color(color)
        .field(EmbedFieldBuilder::new("User", format!("<@{}>", message.author.id)).inline())
        .field(EmbedFieldBuilder::new("Channel", format!("<#{}>", message.channel_id)).inline())
        .field(EmbedFieldBuilder::new("Reason", violation.description()))
        .field(EmbedFieldBuilder::new("Outcome", outcome))
        .field(EmbedFieldBuilder::new(
            "Triggering message",
            truncate(&message.content, 1000),
        ))
        .build()
}

/// Builds the interactive setup panel opened by `/setup`.
pub fn setup_panel(
    config: Option<&GuildConfig>,
    notice: Option<&str>,
    warnings: &[String],
) -> Embed {
    let initialized = config.is_some();
    let setup_status = if initialized {
        "Ready"
    } else {
        "Needs defaults"
    };
    let warnings_text = if warnings.is_empty() {
        "No quick-check findings shown yet.".to_string()
    } else {
        truncate(
            &warnings
                .iter()
                .map(|warning| format!("- {warning}"))
                .collect::<Vec<_>>()
                .join("\n"),
            1024,
        )
    };
    let description = notice.unwrap_or(if initialized {
        "Choose the channel and roles Blackwall should use."
    } else {
        "Create defaults to configure the first log channel and security roles."
    });

    EmbedBuilder::new()
        .title("Blackwall setup")
        .description(description)
        .color(if initialized {
            COLOR_SUCCESS
        } else {
            COLOR_NEUTRAL
        })
        .field(EmbedFieldBuilder::new("Setup status", setup_status).inline())
        .field(
            EmbedFieldBuilder::new(
                "Log channel",
                config
                    .and_then(|config| config.log_channel_id)
                    .map(|id| format!("<#{id}>"))
                    .unwrap_or_else(|| "Not configured".to_string()),
            )
            .inline(),
        )
        .field(
            EmbedFieldBuilder::new(
                "Verified role",
                config
                    .and_then(|config| config.verified_role_id)
                    .map(|id| format!("<@&{id}>"))
                    .unwrap_or_else(|| "Not configured".to_string()),
            )
            .inline(),
        )
        .field(
            EmbedFieldBuilder::new(
                "Quarantine role",
                config
                    .and_then(|config| config.quarantine_role_id)
                    .map(|id| format!("<@&{id}>"))
                    .unwrap_or_else(|| "Not configured".to_string()),
            )
            .inline(),
        )
        .field(
            EmbedFieldBuilder::new(
                "Trusted role",
                config
                    .and_then(|config| config.trusted_role_id)
                    .map(|id| format!("<@&{id}>"))
                    .unwrap_or_else(|| {
                        "Not configured — bot-add gate and invite filtering exempt no one yet"
                            .to_string()
                    }),
            )
            .inline(),
        )
        .field(EmbedFieldBuilder::new(
            "Active protections",
            "- Anti-scam, anti-spam, and anti-invite\n- Anti-raid lockdown\n- Anti-nuke response\n\
            - Bot-add gate (needs a trusted role)",
        ))
        .field(EmbedFieldBuilder::new("Quick check", warnings_text))
        .build()
}

/// Builds the public panel users click to start Discord OAuth verification.
pub fn verify_panel() -> Embed {
    EmbedBuilder::new()
        .title("Verify your account")
        .description(
            "Click the Verify button below to confirm your Discord account with Blackwall. \
            After Discord authorizes the request, Blackwall will grant the server's Verified role.",
        )
        .color(COLOR_SUCCESS)
        .field(EmbedFieldBuilder::new(
            "Requested from Discord",
            "Your username and user ID through the identify OAuth scope.",
        ))
        .build()
}

/// Builds the log embed sent after a best-effort attempt to add a newly
/// verified user to the official support server (only attempted when the
/// verify page disclosed and requested it — see `web::routes`).
pub fn support_join_result(user: &DiscordUser, succeeded: bool) -> Embed {
    let (title, color, description) = if succeeded {
        (
            "Support-server join succeeded",
            COLOR_SUCCESS,
            "User was added to (or was already a member of) the official support server.",
        )
    } else {
        (
            "Support-server join skipped",
            COLOR_NEUTRAL,
            "Blackwall could not add this user to the official support server. Verification \
            still succeeded — this doesn't need any action.",
        )
    };

    EmbedBuilder::new()
        .title(title)
        .color(color)
        .field(EmbedFieldBuilder::new("User", format!("<@{}>", user.id)).inline())
        .field(EmbedFieldBuilder::new("Result", description))
        .build()
}

/// Builds the log embed sent when a join window trips the anti-raid
/// filter. Lists a timeline of the recent joins that triggered it
/// (capped to keep the embed field under Discord's length limit) rather
/// than just saying "a raid happened."
pub fn raid_detected(
    violation: &RaidViolation,
    window: &[JoinRecord],
    timed_out_count: usize,
) -> Embed {
    const MAX_TIMELINE_ENTRIES: usize = 10;

    let mut timeline = window
        .iter()
        .rev()
        .take(MAX_TIMELINE_ENTRIES)
        .map(|record| {
            format!(
                "<@{}> ({}){}",
                record.user_id,
                record.username,
                suspicion_note(record)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    if window.len() > MAX_TIMELINE_ENTRIES {
        timeline.push_str(&format!(
            "\n…and {} more join(s) in this window.",
            window.len() - MAX_TIMELINE_ENTRIES
        ));
    }

    EmbedBuilder::new()
        .title("Raid detected — lockdown engaged")
        .color(COLOR_DANGER)
        .field(EmbedFieldBuilder::new("Trigger", violation.description()))
        .field(EmbedFieldBuilder::new(
            "Response",
            format!(
                "Every text channel was locked (see below for details) and {timed_out_count} \
                suspicious joiner(s) were timed out for 24 hours."
            ),
        ))
        .field(EmbedFieldBuilder::new("Recent joins", timeline))
        .build()
}

/// Builds the "why this join was flagged" suffix for one raid-timeline
/// entry — specific ("no avatar, account created 2 day(s) ago") rather
/// than just "suspicious", since staff reviewing a raid want to know
/// which signal actually fired.
fn suspicion_note(record: &JoinRecord) -> String {
    if !record.is_suspicious {
        return String::new();
    }

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let account_age_days =
        (now_secs - (record.account_created_at.as_micros() / 1_000_000)).max(0) / (60 * 60 * 24);

    let mut reasons = Vec::new();
    if !record.has_avatar {
        reasons.push("no avatar".to_string());
    }
    if account_age_days < 7 {
        reasons.push(format!("account created {account_age_days} day(s) ago"));
    }

    format!(" — {}", reasons.join(", "))
}

/// Builds the log embed sent for a possible ban/kick-evasion join — a
/// single suspicious account joining shortly after a removal, too small
/// on its own to trip the full raid detector. Uses the neutral color (not
/// danger) deliberately: this is a flag for staff to look at, not a
/// confirmed incident Blackwall already acted on.
pub fn possible_evasion(record: &JoinRecord, seconds_since_removal: u64) -> Embed {
    EmbedBuilder::new()
        .title("Possible ban/kick evasion")
        .color(COLOR_NEUTRAL)
        .field(EmbedFieldBuilder::new(
            "Joined",
            format!(
                "<@{}> ({}){}",
                record.user_id,
                record.username,
                suspicion_note(record)
            ),
        ))
        .field(EmbedFieldBuilder::new(
            "Why this was flagged",
            format!(
                "A ban or kick happened in this server {}, and this join looked \
                individually suspicious in that same window. This is a timing \
                correlation only: Blackwall does not use IP addresses or device \
                fingerprints, so it cannot confirm this is the same person. No \
                action was taken automatically.",
                format_duration_ago(seconds_since_removal)
            ),
        ))
        .build()
}

/// Renders a seconds count as "3 minute(s) ago" / "1 hour(s) ago" —
/// precise-to-the-second would read as falsely exact for something
/// that's already only an approximate correlation.
fn format_duration_ago(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds} second(s) ago")
    } else if seconds < 60 * 60 {
        format!("{} minute(s) ago", seconds / 60)
    } else {
        format!("{} hour(s) ago", seconds / (60 * 60))
    }
}

/// Builds the log embed sent when a suspected nuke attempt is detected
/// and responded to.
pub fn nuke_detected(
    violation: &NukeViolation,
    actor_id: Id<UserMarker>,
    roles_removed: usize,
    quarantined: bool,
    channels_locked: usize,
) -> Embed {
    EmbedBuilder::new()
        .title("Nuke attempt detected")
        .color(COLOR_DANGER)
        .field(EmbedFieldBuilder::new("Actor", format!("<@{actor_id}>")).inline())
        .field(EmbedFieldBuilder::new("Trigger", violation.description()))
        .field(EmbedFieldBuilder::new(
            "Response",
            format!(
                "Stripped {roles_removed} dangerous role(s). Quarantined: {}. Locked {channels_locked} \
                channel(s). The server owner was alerted by DM.",
                if quarantined { "yes" } else { "no (no quarantine role configured — run /setup)" }
            ),
        ))
        .build()
}

/// Builds the log embed sent for a manual `/ban`, `/kick`, `/timeout`, or
/// `/warn` command — the staff-triggered counterpart to the automated
/// detectors' own log embeds above.
pub fn manual_moderation_action(
    action_label: &str,
    target_id: Id<UserMarker>,
    moderator_id: Option<Id<UserMarker>>,
    reason: Option<&str>,
    succeeded: bool,
    outcome_label: &str,
) -> Embed {
    EmbedBuilder::new()
        .title(format!("{action_label} — manual moderation action"))
        .color(if succeeded { COLOR_SUCCESS } else { COLOR_DANGER })
        .field(EmbedFieldBuilder::new("Target", format!("<@{target_id}>")).inline())
        .field(
            EmbedFieldBuilder::new(
                "Moderator",
                moderator_id
                    .map(|id| format!("<@{id}>"))
                    .unwrap_or_else(|| "Unknown".to_string()),
            )
            .inline(),
        )
        .field(EmbedFieldBuilder::new("Reason", reason.unwrap_or("No reason given")))
        .field(EmbedFieldBuilder::new("Outcome", outcome_label))
        .build()
}

/// Builds the log embed sent when the trusted-role bot-add gate reacts to
/// a bot/app added by someone without the configured trusted role.
pub fn unauthorized_bot_add(actor_id: Id<UserMarker>, bot_kicked: bool, roles_removed: usize) -> Embed {
    EmbedBuilder::new()
        .title("Unauthorized bot add")
        .color(COLOR_DANGER)
        .field(EmbedFieldBuilder::new("Added by", format!("<@{actor_id}>")).inline())
        .field(
            EmbedFieldBuilder::new(
                "Response",
                format!(
                    "Bot removed: {}. Stripped {roles_removed} dangerous role(s) from the \
                    adding user.",
                    if bot_kicked { "yes" } else { "no — check the bot's Kick Members permission" }
                ),
            ),
        )
        .build()
}

/// Builds the log embed sent when `/lockdown` (or an automatic raid
/// response) engages.
pub fn lockdown_engaged(report: &LockdownReport, triggered_by_raid: bool) -> Embed {
    let title = if triggered_by_raid {
        "Lockdown engaged automatically (raid response)"
    } else {
        "Lockdown engaged"
    };

    EmbedBuilder::new()
        .title(title)
        .color(COLOR_DANGER)
        .field(EmbedFieldBuilder::new(
            "Channels locked",
            report.channels_locked.to_string(),
        ))
        .field(EmbedFieldBuilder::new(
            "Failed",
            report.channels_failed.to_string(),
        ))
        .build()
}

/// Builds the log embed sent when `/unlockdown` restores channel
/// permissions.
pub fn lockdown_reverted(report: &UnlockdownReport) -> Embed {
    EmbedBuilder::new()
        .title("Lockdown lifted")
        .color(COLOR_SUCCESS)
        .field(EmbedFieldBuilder::new(
            "Channels restored",
            report.channels_restored.to_string(),
        ))
        .field(EmbedFieldBuilder::new(
            "Failed",
            report.channels_failed.to_string(),
        ))
        .build()
}

/// Discord embed fields have a length limit; this keeps us comfortably
/// under it even for very long messages instead of letting the API call
/// fail outright.
fn truncate(content: &str, max_chars: usize) -> String {
    if content.is_empty() {
        return "*(no text content)*".to_string();
    }

    if content.chars().count() <= max_chars {
        return content.to_string();
    }

    let mut truncated: String = content.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}
