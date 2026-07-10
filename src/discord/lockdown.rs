use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionContextType};
use twilight_model::channel::message::{Embed, MessageFlags};
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use twilight_util::builder::InteractionResponseDataBuilder;
use twilight_util::builder::command::CommandBuilder;

use crate::actions::lockdown;
use crate::discord::embeds;
use crate::state::AppState;
use crate::storage::models;

/// Both `/lockdown` and `/unlockdown` — kept in one file since they're a
/// matched pair that share the same guard rails and reply shape.
pub fn commands() -> Vec<Command> {
    vec![
        CommandBuilder::new(
            "lockdown",
            "Lock every text channel: stop @everyone from sending messages",
            CommandType::ChatInput,
        )
        .contexts([InteractionContextType::Guild])
        .build(),
        CommandBuilder::new(
            "unlockdown",
            "Undo /lockdown, restoring each channel's exact prior permissions",
            CommandType::ChatInput,
        )
        .contexts([InteractionContextType::Guild])
        .build(),
    ]
}

pub async fn handle_lockdown(interaction: &Interaction, state: &AppState) {
    let Some(guild_id) = interaction.guild_id else {
        respond(
            interaction,
            state,
            "This command can only be used in a server.",
        )
        .await;
        return;
    };

    if !invoker_has_manage_guild(interaction) {
        respond(
            interaction,
            state,
            "You need the **Manage Server** permission to run `/lockdown`.",
        )
        .await;
        return;
    }

    match lockdown::engage(&state.http, &state.db, guild_id).await {
        Ok(report) => {
            respond(
                interaction,
                state,
                &format!(
                    "Lockdown engaged. Locked {} channel(s), {} failed.",
                    report.channels_locked, report.channels_failed
                ),
            )
            .await;

            if let Err(source) = models::record_security_event(
                &state.db,
                guild_id,
                interaction_invoker_id(interaction),
                "lockdown_engaged",
                "high",
                &format!(
                    "/lockdown run manually. Locked {} channel(s), {} failed.",
                    report.channels_locked, report.channels_failed
                ),
            )
            .await
            {
                tracing::error!(?source, %guild_id, "failed to record lockdown security event");
            }

            send_log_embed(state, guild_id, embeds::lockdown_engaged(&report, false)).await;
        }
        Err(source) => {
            tracing::error!(?source, %guild_id, "failed to fetch channels for /lockdown");
            respond(
                interaction,
                state,
                "Couldn't load this server's channels from Discord — please try again.",
            )
            .await;
        }
    }
}

pub async fn handle_unlockdown(interaction: &Interaction, state: &AppState) {
    let Some(guild_id) = interaction.guild_id else {
        respond(
            interaction,
            state,
            "This command can only be used in a server.",
        )
        .await;
        return;
    };

    if !invoker_has_manage_guild(interaction) {
        respond(
            interaction,
            state,
            "You need the **Manage Server** permission to run `/unlockdown`.",
        )
        .await;
        return;
    }

    let report = lockdown::revert(&state.http, &state.db, guild_id).await;
    respond(
        interaction,
        state,
        &format!(
            "Lockdown lifted. Restored {} channel(s), {} failed.",
            report.channels_restored, report.channels_failed
        ),
    )
    .await;

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        interaction_invoker_id(interaction),
        "lockdown_lifted",
        "info",
        &format!(
            "/unlockdown run manually. Restored {} channel(s), {} failed.",
            report.channels_restored, report.channels_failed
        ),
    )
    .await
    {
        tracing::error!(?source, %guild_id, "failed to record unlockdown security event");
    }

    send_log_embed(state, guild_id, embeds::lockdown_reverted(&report)).await;
}

/// The invoking member's user ID, if Discord included it. `PartialMember`
/// only optionally carries `user` in its type — in practice this command
/// is guild-only and Discord always includes it, but `record_security_event`
/// already accepts `Option`, so there's no need to invent a placeholder ID
/// for the type-level possibility it's missing.
fn interaction_invoker_id(interaction: &Interaction) -> Option<Id<UserMarker>> {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.user.as_ref())
        .map(|user| user.id)
}

fn invoker_has_manage_guild(interaction: &Interaction) -> bool {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.permissions)
        .is_some_and(|permissions| {
            permissions.contains(Permissions::MANAGE_GUILD)
                || permissions.contains(Permissions::ADMINISTRATOR)
        })
}

async fn send_log_embed(state: &AppState, guild_id: Id<GuildMarker>, embed: Embed) {
    let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await else {
        return;
    };

    if let Err(source) = state
        .http
        .create_message(log_channel_id)
        .embeds(&[embed])
        .await
    {
        tracing::error!(?source, %guild_id, "failed to send lockdown log embed");
    }
}

async fn respond(interaction: &Interaction, state: &AppState, content: &str) {
    let mut data = InteractionResponseDataBuilder::new()
        .content(content)
        .build();
    data.flags = Some(MessageFlags::EPHEMERAL);

    let response = InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(data),
    };

    let result = state
        .http
        .interaction(state.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await;

    if let Err(source) = result {
        tracing::error!(
            ?source,
            "failed to respond to /lockdown or /unlockdown interaction"
        );
    }
}
