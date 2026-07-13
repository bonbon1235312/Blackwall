use twilight_http::request::AuditLogReason;
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{Interaction, InteractionContextType, InteractionData};
use twilight_model::channel::message::MessageFlags;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_model::id::{
    marker::{GuildMarker, UserMarker},
    Id,
};
use twilight_util::builder::command::{CommandBuilder, IntegerBuilder, StringBuilder, UserBuilder};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::discord::embeds;
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

pub async fn handle_ban(interaction: &Interaction, state: &AppState) {
    let Some((guild_id, target_id, reason)) = required_target(interaction) else {
        return;
    };

    if !invoker_has(interaction, Permissions::BAN_MEMBERS) {
        respond(interaction, state, "You need the **Ban Members** permission to do that.").await;
        return;
    }

    if is_owner(state, guild_id, target_id).await {
        respond(interaction, state, "Discord does not allow moderating the server owner.").await;
        return;
    }

    let mut request = state.http.create_ban(guild_id, target_id);
    if let Some(reason) = reason.as_deref() {
        request = request.reason(reason);
    }

    let outcome = request.await;
    let succeeded = outcome.is_ok();
    if let Err(source) = outcome {
        tracing::error!(?source, %guild_id, %target_id, "failed to ban member via /ban");
    }

    finish(
        interaction,
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

pub async fn handle_kick(interaction: &Interaction, state: &AppState) {
    let Some((guild_id, target_id, reason)) = required_target(interaction) else {
        return;
    };

    if !invoker_has(interaction, Permissions::KICK_MEMBERS) {
        respond(interaction, state, "You need the **Kick Members** permission to do that.").await;
        return;
    }

    if is_owner(state, guild_id, target_id).await {
        respond(interaction, state, "Discord does not allow moderating the server owner.").await;
        return;
    }

    let mut request = state.http.remove_guild_member(guild_id, target_id);
    if let Some(reason) = reason.as_deref() {
        request = request.reason(reason);
    }

    let outcome = request.await;
    let succeeded = outcome.is_ok();
    if let Err(source) = outcome {
        tracing::error!(?source, %guild_id, %target_id, "failed to kick member via /kick");
    }

    finish(
        interaction,
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

pub async fn handle_timeout(interaction: &Interaction, state: &AppState) {
    let Some((guild_id, target_id, reason)) = required_target(interaction) else {
        return;
    };

    if !invoker_has(interaction, Permissions::MODERATE_MEMBERS) {
        respond(
            interaction,
            state,
            "You need the **Timeout Members** permission to do that.",
        )
        .await;
        return;
    }

    if is_owner(state, guild_id, target_id).await {
        respond(interaction, state, "Discord does not allow moderating the server owner.").await;
        return;
    }

    let Some(minutes) = integer_option(interaction, OPT_DURATION_MINUTES) else {
        respond(interaction, state, "A timeout duration is required.").await;
        return;
    };

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
    if let Err(source) = outcome {
        tracing::error!(?source, %guild_id, %target_id, "failed to time out member via /timeout");
    }

    finish(
        interaction,
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

pub async fn handle_warn(interaction: &Interaction, state: &AppState) {
    let Some((guild_id, target_id, reason)) = required_target(interaction) else {
        return;
    };
    let Some(reason) = reason else {
        respond(interaction, state, "A reason is required to warn someone.").await;
        return;
    };

    if !invoker_has(interaction, Permissions::MODERATE_MEMBERS) {
        respond(interaction, state, "You need the **Timeout Members** permission to do that.")
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
        interaction,
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
    interaction: &Interaction,
    state: &AppState,
    guild_id: Id<GuildMarker>,
    target_id: Id<UserMarker>,
    reason: Option<&str>,
    succeeded: bool,
    event_type: &str,
    action_label: &str,
    outcome_label: &str,
) {
    let moderator_id = interaction
        .member
        .as_ref()
        .and_then(|member| member.user.as_ref())
        .map(|user| user.id);

    let description = format!(
        "{action_label} <@{target_id}>{}. {}",
        reason.map(|r| format!(" ({r})")).unwrap_or_default(),
        if succeeded {
            outcome_label.to_string()
        } else {
            format!("{action_label} failed — check the bot's permissions and role position.")
        }
    );

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(target_id),
        event_type,
        if succeeded { "medium" } else { "low" },
        &description,
    )
    .await
    {
        tracing::error!(?source, %guild_id, "failed to record moderation-command security event");
    }

    respond(interaction, state, &description).await;

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

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, %guild_id, "failed to send moderation-command log embed");
    }
}

/// Pulls `(guild_id, target user, optional reason)` out of the invoking
/// interaction. Returns `None` (and has already replied) if this wasn't
/// invoked in a guild — every other precondition is checked by the
/// caller, since the required permission differs per command.
fn required_target(
    interaction: &Interaction,
) -> Option<(Id<GuildMarker>, Id<UserMarker>, Option<String>)> {
    let guild_id = interaction.guild_id?;
    let target_id = user_option(interaction, OPT_USER)?;
    let reason = string_option(interaction, OPT_REASON);

    Some((guild_id, target_id, reason))
}

fn command_options(interaction: &Interaction) -> &[twilight_model::application::interaction::application_command::CommandDataOption] {
    match interaction.data.as_ref() {
        Some(InteractionData::ApplicationCommand(command)) => &command.options,
        _ => &[],
    }
}

fn user_option(interaction: &Interaction, name: &str) -> Option<Id<UserMarker>> {
    command_options(interaction).iter().find_map(|option| {
        if option.name == name {
            if let CommandOptionValue::User(id) = option.value {
                return Some(id);
            }
        }
        None
    })
}

fn string_option(interaction: &Interaction, name: &str) -> Option<String> {
    command_options(interaction).iter().find_map(|option| {
        if option.name == name {
            if let CommandOptionValue::String(value) = &option.value {
                return Some(value.clone());
            }
        }
        None
    })
}

fn integer_option(interaction: &Interaction, name: &str) -> Option<i64> {
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

fn invoker_has(interaction: &Interaction, permission: Permissions) -> bool {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.permissions)
        .is_some_and(|permissions| {
            permissions.contains(permission) || permissions.contains(Permissions::ADMINISTRATOR)
        })
}

async fn respond(interaction: &Interaction, state: &AppState, content: &str) {
    let data = InteractionResponseDataBuilder::new()
        .content(content)
        .flags(MessageFlags::EPHEMERAL)
        .build();
    let response = InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(data),
    };

    if let Err(source) = state
        .http
        .interaction(state.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await
    {
        tracing::error!(?source, "failed to respond to moderation-command interaction");
    }
}
