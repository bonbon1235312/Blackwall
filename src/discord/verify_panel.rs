use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionContextType};
use twilight_model::channel::message::component::ButtonStyle;
use twilight_model::channel::message::{Component, MessageFlags};
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::InteractionResponseDataBuilder;
use twilight_util::builder::command::CommandBuilder;
use twilight_util::builder::message::{ActionRowBuilder, ButtonBuilder};

use crate::discord::embeds;
use crate::state::AppState;

pub fn command() -> Command {
    CommandBuilder::new(
        "verify-panel",
        "Post a public verification panel in this channel",
        CommandType::ChatInput,
    )
    .contexts([InteractionContextType::Guild])
    .build()
}

pub async fn handle(interaction: &Interaction, state: &AppState) {
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
            "You need the **Manage Server** permission to post a verification panel.",
        )
        .await;
        return;
    }

    if state.discord_client_secret.is_none() {
        respond(
            interaction,
            state,
            "Verification is not configured yet. Set `DISCORD_CLIENT_SECRET` before posting a panel.",
        )
        .await;
        return;
    }

    let Some(channel_id) = interaction.channel.as_ref().map(|channel| channel.id) else {
        respond(
            interaction,
            state,
            "Couldn't tell which channel this command was run in.",
        )
        .await;
        return;
    };

    let verify_url = format!("{}/verify?guild_id={guild_id}", state.public_base_url);
    let button = ButtonBuilder::new(ButtonStyle::Link)
        .label("Verify")
        .url(verify_url)
        .build();
    let action_row = ActionRowBuilder::new().component(button).build();
    let panel_embed = embeds::verify_panel();
    let embeds = [panel_embed];
    let components = [Component::ActionRow(action_row)];

    if let Err(source) = state
        .http
        .create_message(channel_id)
        .embeds(&embeds)
        .components(&components)
        .await
    {
        tracing::error!(?source, %guild_id, %channel_id, "failed to post verification panel");
        respond(
            interaction,
            state,
            "Couldn't post the verification panel. Check that Blackwall can send messages in this channel.",
        )
        .await;
        return;
    }

    respond(interaction, state, "Verification panel posted.").await;
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
        tracing::error!(?source, "failed to respond to /verify-panel interaction");
    }
}
