use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::{Component, Embed, MessageFlags};
use twilight_model::channel::Message;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;
use twilight_util::builder::InteractionResponseDataBuilder;
use twilight_util::permission_calculator::PermissionCalculator;

use crate::state::AppState;

/// Abstracts over "a slash command interaction" vs. a `!`/`?` prefix
/// command message. Every command's actual logic — permission gate, do
/// the work, reply — is written once against this, rather than once per
/// entry point. The two sources differ only in how the invoker/guild/
/// channel are read and how a reply gets sent, never in what a command
/// does.
pub enum CommandSource<'a> {
    Interaction(&'a Interaction),
    Message(&'a Message),
}

impl CommandSource<'_> {
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        match self {
            Self::Interaction(interaction) => interaction.guild_id,
            Self::Message(message) => message.guild_id,
        }
    }

    pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        match self {
            Self::Interaction(interaction) => interaction.channel.as_ref().map(|c| c.id),
            Self::Message(message) => Some(message.channel_id),
        }
    }

    pub fn invoker_id(&self) -> Option<Id<UserMarker>> {
        match self {
            Self::Interaction(interaction) => interaction
                .member
                .as_ref()
                .and_then(|member| member.user.as_ref())
                .map(|user| user.id),
            Self::Message(message) => Some(message.author.id),
        }
    }

    /// The invoker's guild-level permissions. Free for a slash command —
    /// Discord includes it directly on the interaction. For a prefix
    /// command, reads the guild's role-permission table from
    /// `state.role_cache` instead of Discord directly — the member's own
    /// role *assignments* are already inline on `message.member`, so the
    /// only thing that ever needed a network round-trip was the
    /// role-ID-to-permissions mapping, and that's cached.
    pub async fn invoker_permissions(&self, state: &AppState) -> Permissions {
        match self {
            Self::Interaction(interaction) => interaction
                .member
                .as_ref()
                .and_then(|member| member.permissions)
                .unwrap_or_else(Permissions::empty),
            Self::Message(message) => {
                let (Some(guild_id), Some(member)) = (message.guild_id, message.member.as_ref())
                else {
                    return Permissions::empty();
                };

                let Some(snapshot) = state.role_cache.get(&state.http, &state.db, guild_id).await
                else {
                    return Permissions::empty();
                };

                let member_roles: Vec<(Id<RoleMarker>, Permissions)> = member
                    .roles
                    .iter()
                    .filter_map(|role_id| {
                        snapshot
                            .roles
                            .get(role_id)
                            .map(|permissions| (*role_id, *permissions))
                    })
                    .collect();

                let mut calculator = PermissionCalculator::new(
                    guild_id,
                    message.author.id,
                    snapshot.everyone_permissions,
                    &member_roles,
                );
                if let Some(owner_id) = snapshot.owner_id {
                    calculator = calculator.owner_id(owner_id);
                }

                calculator.root()
            }
        }
    }

    pub async fn reply(&self, state: &AppState, content: &str) {
        match self {
            Self::Interaction(interaction) => {
                let mut data = InteractionResponseDataBuilder::new()
                    .content(content)
                    .build();
                data.flags = Some(MessageFlags::EPHEMERAL);
                send_interaction_response(interaction, state, data).await;
            }
            Self::Message(message) => {
                if let Err(source) = state
                    .http
                    .create_message(message.channel_id)
                    .content(content)
                    .await
                {
                    tracing::error!(?source, "failed to reply to prefix command");
                }
            }
        }
    }

    pub async fn reply_with_embed(&self, state: &AppState, embed: Embed) {
        match self {
            Self::Interaction(interaction) => {
                let mut data = InteractionResponseDataBuilder::new().embeds([embed]).build();
                data.flags = Some(MessageFlags::EPHEMERAL);
                send_interaction_response(interaction, state, data).await;
            }
            Self::Message(message) => {
                if let Err(source) = state
                    .http
                    .create_message(message.channel_id)
                    .embeds(&[embed])
                    .await
                {
                    tracing::error!(?source, "failed to reply to prefix command with an embed");
                }
            }
        }
    }

    /// Replies with an embed plus interactive components (buttons/select
    /// menus) — the setup panel and the verify panel both need this.
    /// Discord routes a component click as its own fresh interaction
    /// referencing the message it's attached to either way, so a panel
    /// posted from a prefix command supports the exact same clicks as one
    /// posted from a slash command.
    pub async fn reply_with_panel(&self, state: &AppState, embed: Embed, components: Vec<Component>) {
        match self {
            Self::Interaction(interaction) => {
                let data = InteractionResponseDataBuilder::new()
                    .embeds([embed])
                    .components(components)
                    .flags(MessageFlags::EPHEMERAL)
                    .build();
                send_interaction_response(interaction, state, data).await;
            }
            Self::Message(message) => {
                if let Err(source) = state
                    .http
                    .create_message(message.channel_id)
                    .embeds(&[embed])
                    .components(&components)
                    .await
                {
                    tracing::error!(?source, "failed to post a panel from a prefix command");
                }
            }
        }
    }
}

async fn send_interaction_response(
    interaction: &Interaction,
    state: &AppState,
    data: InteractionResponseData,
) {
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
        tracing::error!(?source, "failed to respond to interaction");
    }
}
