use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::discord::source::CommandSource;
use crate::discord::{backup, config, lockdown, moderate, security_score, setup, verify_panel};
use crate::state::AppState;

/// Handles a single interaction: slash commands and the setup panel's
/// message components.
///
/// Every interaction must be responded to within 3 seconds or Discord shows
/// the user an error. Simple commands like `/ping` reply directly here;
/// commands with real work to do (like `/setup`) get their own module and
/// build their own response.
pub async fn handle(interaction: Interaction, state: &AppState) {
    match interaction.data.as_ref() {
        Some(InteractionData::ApplicationCommand(command)) => {
            let source = CommandSource::Interaction(&interaction);
            match command.name.as_str() {
                "ping" => respond_pong(&interaction, state).await,
                "setup" => setup::handle_command(&source, state).await,
                "verify-panel" => verify_panel::handle(&source, state).await,
                "lockdown" => lockdown::handle_lockdown(&source, state).await,
                "unlockdown" => lockdown::handle_unlockdown(&source, state).await,
                "security-score" => security_score::handle(&source, state).await,
                "backup" => backup::handle_backup(&source, state).await,
                "restore" => backup::handle_restore(&source, state).await,
                "config" => config::handle(&source, state, None).await,
                "ban" => moderate::handle_ban(&source, state, None).await,
                "kick" => moderate::handle_kick(&source, state, None).await,
                "timeout" => moderate::handle_timeout(&source, state, None).await,
                "warn" => moderate::handle_warn(&source, state, None).await,
                other => tracing::warn!(command = other, "received unknown slash command"),
            }
        }
        Some(InteractionData::MessageComponent(component))
            if setup::handles_component(&component.custom_id) =>
        {
            setup::handle_component(&interaction, component, state).await;
        }
        Some(InteractionData::MessageComponent(component)) => {
            tracing::warn!(custom_id = %component.custom_id, "received unknown message component");
        }
        _ => {}
    }
}

async fn respond_pong(interaction: &Interaction, state: &AppState) {
    let response = InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(
            InteractionResponseDataBuilder::new()
                .content("Pong! Blackwall is online.")
                .build(),
        ),
    };

    let result = state
        .http
        .interaction(state.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await;

    if let Err(source) = result {
        tracing::error!(?source, "failed to respond to interaction");
    }
}
