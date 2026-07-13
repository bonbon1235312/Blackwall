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
use crate::storage::models;

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
    /// Discord includes it directly on the interaction. Costs one API
    /// call for a prefix command (fetches the guild's roles to compute
    /// it via `PermissionCalculator`) — acceptable since this only runs
    /// on a deliberate command invocation, nothing like the per-message
    /// hot path `gateway.rs`'s scam/spam/invite checks run on.
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

                let Ok(roles_response) = state.http.roles(guild_id).await else {
                    return Permissions::empty();
                };
                let Ok(roles) = roles_response.model().await else {
                    return Permissions::empty();
                };

                let everyone_role = roles
                    .iter()
                    .find(|role| role.id.get() == guild_id.get())
                    .map_or_else(Permissions::empty, |role| role.permissions);

                let member_roles: Vec<(Id<RoleMarker>, Permissions)> = member
                    .roles
                    .iter()
                    .filter_map(|role_id| {
                        roles
                            .iter()
                            .find(|role| role.id == *role_id)
                            .map(|role| (*role_id, role.permissions))
                    })
                    .collect();

                // Blackwall only knows the owner once `/setup` has run at
                // least once. A slash command doesn't need this fallback —
                // Discord computes the invoker's real effective
                // permissions (owner status included) server-side and
                // sends it directly on the interaction — but this
                // hand-rolled prefix-command calculator needs an owner ID
                // to apply that same rule, so fall back to asking Discord
                // directly on a guild Blackwall hasn't recorded yet.
                let owner_id = match models::get_owner_id(&state.db, guild_id).await {
                    Some(owner_id) => Some(owner_id),
                    None => match state.http.guild(guild_id).await {
                        Ok(response) => response.model().await.ok().map(|guild| guild.owner_id),
                        Err(_) => None,
                    },
                };

                let mut calculator = PermissionCalculator::new(
                    guild_id,
                    message.author.id,
                    everyone_role,
                    &member_roles,
                );
                if let Some(owner_id) = owner_id {
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
