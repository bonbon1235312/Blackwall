use twilight_http::request::AuditLogReason;
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{InteractionContextType, InteractionData};
use twilight_model::guild::Permissions;
use twilight_model::id::{
    marker::{GuildMarker, UserMarker},
    Id,
};
use twilight_util::builder::command::{CommandBuilder, IntegerBuilder, StringBuilder, UserBuilder};

use crate::discord::embeds;
use crate::discord::source::CommandSource;
use crate::moderation::raid;
use crate::state::AppState;
use crate::storage::models;

const OPT_USER: &str = "user";
const OPT_REASON: &str = "reason";
const OPT_DURATION_MINUTES: &str = "duration-minutes";

pub fn commands() -> Vec<Command> {
    vec![
        CommandBuilder::new("ban", "Ban a member from this server", CommandType::ChatInput)
            .contexts([InteractionContextType::Guild])
            .option(UserBuilder::new(OPT_USER, "Who to ban").required(true))
            .option(StringBuilder::new(OPT_REASON, "Why they're being banned").required(false))
            .build(),
        CommandBuilder::new("kick", "Kick a member from this server", CommandType::ChatInput)
            .contexts([InteractionContextType::Guild])
            .option(UserBuilder::new(OPT_USER, "Who to kick").required(true))
            .option(StringBuilder::new(OPT_REASON, "Why they're being kicked").required(false))
            .build(),
        CommandBuilder::new("timeout", "Time out a member", CommandType::ChatInput)
            .contexts([InteractionContextType::Guild])
            .option(UserBuilder::new(OPT_USER, "Who to time out").required(true))
            .option(
                IntegerBuilder::new(OPT_DURATION_MINUTES, "How long, in minutes")
                    .min_value(1)
                    .max_value(40_320)
                    .required(true),
            )
            .option(StringBuilder::new(OPT_REASON, "Why they're being timed out").required(false))
            .build(),
        CommandBuilder::new("warn", "Send a member a formal warning", CommandType::ChatInput)
            .contexts([InteractionContextType::Guild])
            .option(UserBuilder::new(OPT_USER, "Who to warn").required(true))
            .option(StringBuilder::new(OPT_REASON, "What they're being warned about").required(true))
            .build(),
    ]
}

pub async fn handle_ban(source: &CommandSource<'_>, state: &AppState, prefix_args: Option<&str>) {
    let Some((guild_id, target_id, reason)) = required_target(source, prefix_args) else {
        source
            .reply(state, "Usage: `/ban user:@member [reason]` or `!ban @member [reason]`.")
            .await;
        return;
    };

    if !has(source, state, Permissions::BAN_MEMBERS).await {
        source
            .reply(state, "You need the **Ban Members** permission to do that.")
            .await;
        return;
    }

    if is_owner(state, guild_id, target_id).await {
        source
            .reply(state, "Discord does not allow moderating the server owner.")
            .await;
        return;
    }

    let mut request = state.http.create_ban(guild_id, target_id);
    if let Some(reason) = reason.as_deref() {
        request = request.reason(reason);
    }

    let outcome = request.await;
    let succeeded = outcome.is_ok();
    if let Err(error) = outcome {
        tracing::error!(?error, %guild_id, %target_id, "failed to ban member via /ban");
    }

    finish(
        source,
        state,
        guild_id,
        target_id,
        reason.as_deref(),
        succeeded,
        "member_banned",
        "Ban",
        "Banned",
    )
    .await;
}

pub async fn handle_kick(source: &CommandSource<'_>, state: &AppState, prefix_args: Option<&str>) {
    let Some((guild_id, target_id, reason)) = required_target(source, prefix_args) else {
        source
            .reply(state, "Usage: `/kick user:@member [reason]` or `!kick @member [reason]`.")
            .await;
        return;
    };

    if !has(source, state, Permissions::KICK_MEMBERS).await {
        source
            .reply(state, "You need the **Kick Members** permission to do that.")
            .await;
        return;
    }

    if is_owner(state, guild_id, target_id).await {
        source
            .reply(state, "Discord does not allow moderating the server owner.")
            .await;
        return;
    }

    let mut request = state.http.remove_guild_member(guild_id, target_id);
    if let Some(reason) = reason.as_deref() {
        request = request.reason(reason);
    }

    let outcome = request.await;
    let succeeded = outcome.is_ok();
    if let Err(error) = outcome {
        tracing::error!(?error, %guild_id, %target_id, "failed to kick member via /kick");
    }

    finish(
        source,
        state,
        guild_id,
        target_id,
        reason.as_deref(),
        succeeded,
        "member_kicked",
        "Kick",
        "Kicked",
    )
    .await;
}

pub async fn handle_timeout(source: &CommandSource<'_>, state: &AppState, prefix_args: Option<&str>) {
    let Some((guild_id, target_id, minutes, reason)) = required_target_with_duration(source, prefix_args)
    else {
        source
            .reply(
                state,
                "Usage: `/timeout user:@member duration-minutes:30 [reason]` or \
                `!timeout @member 30 [reason]`.",
            )
            .await;
        return;
    };

    if !has(source, state, Permissions::MODERATE_MEMBERS).await {
        source
            .reply(state, "You need the **Timeout Members** permission to do that.")
            .await;
        return;
    }

    if is_owner(state, guild_id, target_id).await {
        source
            .reply(state, "Discord does not allow moderating the server owner.")
            .await;
        return;
    }

    let until = raid::raid_timeout_until(std::time::Duration::from_secs(minutes as u64 * 60));

    let mut request = state
        .http
        .update_guild_member(guild_id, target_id)
        .communication_disabled_until(Some(until));
    if let Some(reason) = reason.as_deref() {
        request = request.reason(reason);
    }

    let outcome = request.await;
    let succeeded = outcome.is_ok();
    if let Err(error) = outcome {
        tracing::error!(?error, %guild_id, %target_id, "failed to time out member via /timeout");
    }

    finish(
        source,
        state,
        guild_id,
        target_id,
        reason.as_deref(),
        succeeded,
        "member_timed_out",
        "Timeout",
        &format!("Timed out for {minutes} minute(s)"),
    )
    .await;
}

pub async fn handle_warn(source: &CommandSource<'_>, state: &AppState, prefix_args: Option<&str>) {
    let Some((guild_id, target_id, reason)) = required_target(source, prefix_args) else {
        source
            .reply(state, "Usage: `/warn user:@member reason:...` or `!warn @member reason...`.")
            .await;
        return;
    };
    let Some(reason) = reason else {
        source
            .reply(state, "A reason is required to warn someone.")
            .await;
        return;
    };

    if !has(source, state, Permissions::MODERATE_MEMBERS).await {
        source
            .reply(state, "You need the **Timeout Members** permission to do that.")
            .await;
        return;
    }

    // /warn has no Discord moderation API call to make — it's a DM plus a
    // logged event, matching the existing nuke-owner-alert DM pattern
    // rather than any of moderate.rs's other, real API-backed actions.
    let dm_content = format!("**You have been warned in a server you're in.**\n{reason}");
    let dm_sent = match state.http.create_private_channel(target_id).await {
        Ok(response) => match response.model().await {
            Ok(channel) => state
                .http
                .create_message(channel.id)
                .content(&dm_content)
                .await
                .is_ok(),
            Err(_) => false,
        },
        Err(_) => false,
    };

    finish(
        source,
        state,
        guild_id,
        target_id,
        Some(&reason),
        true,
        "member_warned",
        "Warn",
        if dm_sent {
            "Warned (DM delivered)"
        } else {
            "Warned (DM could not be delivered)"
        },
    )
    .await;
}

/// Shared tail for all four commands: replies to the invoking moderator,
/// records a `security_events` row (the case log — reusing the existing
/// table rather than a new structured one), and sends the standard
/// log-channel embed, matching every automated detector's own ending
/// shape.
#[allow(clippy::too_many_arguments)]
async fn finish(
    source: &CommandSource<'_>,
    state: &AppState,
    guild_id: Id<GuildMarker>,
    target_id: Id<UserMarker>,
    reason: Option<&str>,
    succeeded: bool,
    event_type: &str,
    action_label: &str,
    outcome_label: &str,
) {
    let moderator_id = source.invoker_id();

    let description = format!(
        "{action_label} <@{target_id}>{}. {}",
        reason.map(|r| format!(" ({r})")).unwrap_or_default(),
        if succeeded {
            outcome_label.to_string()
        } else {
            format!("{action_label} failed — check the bot's permissions and role position.")
        }
    );

    if let Err(error) = models::record_security_event(
        &state.db,
        guild_id,
        Some(target_id),
        event_type,
        if succeeded { "medium" } else { "low" },
        &description,
    )
    .await
    {
        tracing::error!(?error, %guild_id, "failed to record moderation-command security event");
    }

    source.reply(state, &description).await;

    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    let embed = embeds::manual_moderation_action(
        action_label,
        target_id,
        moderator_id,
        reason,
        succeeded,
        outcome_label,
    );

    if let Err(error) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?error, %guild_id, "failed to send moderation-command log embed");
    }
}

/// Parses `<@123>`, `<@!123>` (Discord mention formats), or a bare
/// numeric ID into a user ID — the three shapes a prefix command's first
/// argument can take when someone types or @-mentions a target.
fn parse_user_token(token: &str) -> Option<Id<UserMarker>> {
    let mention_inner = token
        .strip_prefix("<@")
        .and_then(|rest| rest.strip_suffix('>'));
    let digits = match mention_inner {
        Some(inner) => inner.strip_prefix('!').unwrap_or(inner),
        None => token,
    };

    digits.parse().ok()
}

/// Pulls `(guild_id, target user, optional reason)` for `/ban`, `/kick`,
/// and `/warn` — from the interaction's real options, or from prefix text
/// shaped `<target> [reason...]`.
fn required_target(
    source: &CommandSource<'_>,
    prefix_args: Option<&str>,
) -> Option<(Id<GuildMarker>, Id<UserMarker>, Option<String>)> {
    let guild_id = source.guild_id()?;

    let (target_id, reason) = match source {
        CommandSource::Interaction(interaction) => (
            user_option(interaction, OPT_USER)?,
            string_option(interaction, OPT_REASON),
        ),
        CommandSource::Message(_) => {
            let args = prefix_args?.trim();
            let (target_token, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            let target_id = parse_user_token(target_token)?;
            let reason = rest.trim();
            (target_id, (!reason.is_empty()).then(|| reason.to_owned()))
        }
    };

    Some((guild_id, target_id, reason))
}

/// `(guild_id, target user, timeout minutes, optional reason)`.
type TimeoutTarget = (Id<GuildMarker>, Id<UserMarker>, i64, Option<String>);

/// Same as `required_target`, plus a required duration in minutes —
/// `/timeout`'s own extra option, or a second positional prefix argument
/// (`!timeout <target> <minutes> [reason...]`).
fn required_target_with_duration(
    source: &CommandSource<'_>,
    prefix_args: Option<&str>,
) -> Option<TimeoutTarget> {
    let guild_id = source.guild_id()?;

    let (target_id, minutes, reason) = match source {
        CommandSource::Interaction(interaction) => (
            user_option(interaction, OPT_USER)?,
            integer_option(interaction, OPT_DURATION_MINUTES)?,
            string_option(interaction, OPT_REASON),
        ),
        CommandSource::Message(_) => {
            let args = prefix_args?.trim();
            let mut parts = args.splitn(3, char::is_whitespace);
            let target_id = parse_user_token(parts.next()?)?;
            let minutes: i64 = parts.next()?.parse().ok()?;
            let reason = parts.next().unwrap_or("").trim();
            (target_id, minutes, (!reason.is_empty()).then(|| reason.to_owned()))
        }
    };

    Some((guild_id, target_id, minutes, reason))
}

fn command_options(
    interaction: &twilight_model::application::interaction::Interaction,
) -> &[twilight_model::application::interaction::application_command::CommandDataOption] {
    match interaction.data.as_ref() {
        Some(InteractionData::ApplicationCommand(command)) => &command.options,
        _ => &[],
    }
}

fn user_option(
    interaction: &twilight_model::application::interaction::Interaction,
    name: &str,
) -> Option<Id<UserMarker>> {
    command_options(interaction).iter().find_map(|option| {
        if option.name == name {
            if let CommandOptionValue::User(id) = option.value {
                return Some(id);
            }
        }
        None
    })
}

fn string_option(
    interaction: &twilight_model::application::interaction::Interaction,
    name: &str,
) -> Option<String> {
    command_options(interaction).iter().find_map(|option| {
        if option.name == name {
            if let CommandOptionValue::String(value) = &option.value {
                return Some(value.clone());
            }
        }
        None
    })
}

fn integer_option(
    interaction: &twilight_model::application::interaction::Interaction,
    name: &str,
) -> Option<i64> {
    command_options(interaction).iter().find_map(|option| {
        if option.name == name {
            if let CommandOptionValue::Integer(value) = option.value {
                return Some(value);
            }
        }
        None
    })
}

async fn is_owner(state: &AppState, guild_id: Id<GuildMarker>, user_id: Id<UserMarker>) -> bool {
    models::get_owner_id(&state.db, guild_id).await == Some(user_id)
}

async fn has(source: &CommandSource<'_>, state: &AppState, permission: Permissions) -> bool {
    let permissions = source.invoker_permissions(state).await;
    permissions.contains(permission) || permissions.contains(Permissions::ADMINISTRATOR)
}
