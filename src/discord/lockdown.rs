use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::InteractionContextType;
use twilight_model::channel::message::Embed;
use twilight_model::guild::Permissions;
use twilight_model::id::{Id, marker::GuildMarker};
use twilight_util::builder::command::CommandBuilder;

use crate::actions::lockdown;
use crate::discord::embeds;
use crate::discord::source::CommandSource;
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

pub async fn handle_lockdown(source: &CommandSource<'_>, state: &AppState) {
    let Some(guild_id) = source.guild_id() else {
        source
            .reply(state, "This command can only be used in a server.")
            .await;
        return;
    };

    if !has_manage_guild(source, state).await {
        source
            .reply(
                state,
                "You need the **Manage Server** permission to run `/lockdown`.",
            )
            .await;
        return;
    }

    match lockdown::engage(&state.http, &state.db, guild_id).await {
        Ok(report) => {
            source
                .reply(
                    state,
                    &format!(
                        "Lockdown engaged. Locked {} channel(s), {} failed.",
                        report.channels_locked, report.channels_failed
                    ),
                )
                .await;

            if let Err(source_err) = models::record_security_event(
                &state.db,
                guild_id,
                source.invoker_id(),
                "lockdown_engaged",
                "high",
                &format!(
                    "/lockdown run manually. Locked {} channel(s), {} failed.",
                    report.channels_locked, report.channels_failed
                ),
            )
            .await
            {
                tracing::error!(?source_err, %guild_id, "failed to record lockdown security event");
            }

            send_log_embed(state, guild_id, embeds::lockdown_engaged(&report, false)).await;
        }
        Err(source_err) => {
            tracing::error!(?source_err, %guild_id, "failed to fetch channels for /lockdown");
            source
                .reply(
                    state,
                    "Couldn't load this server's channels from Discord — please try again.",
                )
                .await;
        }
    }
}

pub async fn handle_unlockdown(source: &CommandSource<'_>, state: &AppState) {
    let Some(guild_id) = source.guild_id() else {
        source
            .reply(state, "This command can only be used in a server.")
            .await;
        return;
    };

    if !has_manage_guild(source, state).await {
        source
            .reply(
                state,
                "You need the **Manage Server** permission to run `/unlockdown`.",
            )
            .await;
        return;
    }

    let report = lockdown::revert(&state.http, &state.db, guild_id).await;
    source
        .reply(
            state,
            &format!(
                "Lockdown lifted. Restored {} channel(s), {} failed.",
                report.channels_restored, report.channels_failed
            ),
        )
        .await;

    if let Err(source_err) = models::record_security_event(
        &state.db,
        guild_id,
        source.invoker_id(),
        "lockdown_lifted",
        "info",
        &format!(
            "/unlockdown run manually. Restored {} channel(s), {} failed.",
            report.channels_restored, report.channels_failed
        ),
    )
    .await
    {
        tracing::error!(?source_err, %guild_id, "failed to record unlockdown security event");
    }

    send_log_embed(state, guild_id, embeds::lockdown_reverted(&report)).await;
}

async fn has_manage_guild(source: &CommandSource<'_>, state: &AppState) -> bool {
    let permissions = source.invoker_permissions(state).await;
    permissions.contains(Permissions::MANAGE_GUILD) || permissions.contains(Permissions::ADMINISTRATOR)
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
