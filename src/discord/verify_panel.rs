use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::InteractionContextType;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::Component;
use twilight_model::guild::Permissions;
use twilight_util::builder::command::CommandBuilder;

use crate::discord::embeds;
use crate::discord::source::CommandSource;
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

pub async fn handle(source: &CommandSource<'_>, state: &AppState) {
    let Some(guild_id) = source.guild_id() else {
        source
            .reply(state, "This command can only be used in a server.")
            .await;
        return;
    };

    let permissions = source.invoker_permissions(state).await;
    if !permissions.contains(Permissions::MANAGE_GUILD)
        && !permissions.contains(Permissions::ADMINISTRATOR)
    {
        source
            .reply(
                state,
                "You need the **Manage Server** permission to post a verification panel.",
            )
            .await;
        return;
    }

    if state.discord_client_secret.is_none() {
        source
            .reply(
                state,
                "Verification is not configured yet. Set `DISCORD_CLIENT_SECRET` before posting a panel.",
            )
            .await;
        return;
    }

    let Some(channel_id) = source.channel_id() else {
        source
            .reply(state, "Couldn't tell which channel this command was run in.")
            .await;
        return;
    };

    let verify_url = format!("{}/verify?guild_id={guild_id}", state.public_base_url);
    let button = Button {
        custom_id: None,
        disabled: false,
        emoji: None,
        label: Some("Verify".to_owned()),
        style: ButtonStyle::Link,
        url: Some(verify_url),
        sku_id: None,
    };
    let action_row = ActionRow {
        components: vec![Component::Button(button)],
    };
    let panel_embed = embeds::verify_panel();
    let components = [Component::ActionRow(action_row)];

    // Deliberately NOT `source.reply_with_panel` — that's ephemeral for a
    // slash command (visible only to the invoker), but this panel needs
    // to be a normal, public message every member can see and click.
    if let Err(error) = state
        .http
        .create_message(channel_id)
        .embeds(&[panel_embed])
        .components(&components)
        .await
    {
        tracing::error!(?error, %guild_id, %channel_id, "failed to post verification panel");
        source
            .reply(
                state,
                "Couldn't post the verification panel. Check that Blackwall can send messages in this channel.",
            )
            .await;
        return;
    }

    source.reply(state, "Verification panel posted.").await;
}
