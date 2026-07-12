use serde::{Deserialize, Serialize};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionContextType};
use twilight_model::channel::message::MessageFlags;
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::command::CommandBuilder;
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::state::AppState;
use crate::storage::models;

/// A backed-up role. Deliberately doesn't try to capture permission
/// *overwrites* (per-channel exceptions) — those reference role/member
/// IDs that can't survive a restore anyway (see `handle_restore`'s
/// disclosure). Just the role itself: name, base permissions, color,
/// display settings.
#[derive(Serialize, Deserialize)]
struct RoleBackup {
    name: String,
    permission_bits: u64,
    color: u32,
    hoist: bool,
    mentionable: bool,
}

/// A backed-up channel — text channels and categories only for now. Voice
/// channels, threads, forums etc. aren't covered by this first version.
#[derive(Serialize, Deserialize)]
struct ChannelBackup {
    name: String,
    kind: ChannelType,
    topic: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct GuildBackup {
    guild_name: String,
    roles: Vec<RoleBackup>,
    channels: Vec<ChannelBackup>,
}

pub fn commands() -> Vec<Command> {
    vec![
        CommandBuilder::new(
            "backup",
            "Back up this server's roles and text channels",
            CommandType::ChatInput,
        )
        .contexts([InteractionContextType::Guild])
        .build(),
        CommandBuilder::new(
            "restore",
            "Recreate roles/channels missing since the most recent backup",
            CommandType::ChatInput,
        )
        .contexts([InteractionContextType::Guild])
        .build(),
    ]
}

pub async fn handle_backup(interaction: &Interaction, state: &AppState) {
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
            "You need the **Manage Server** permission to run `/backup`.",
        )
        .await;
        return;
    }

    let Ok(guild_response) = state.http.guild(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server from Discord.",
        )
        .await;
        return;
    };
    let Ok(guild) = guild_response.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back a server we couldn't read.",
        )
        .await;
        return;
    };

    let Ok(roles_response) = state.http.roles(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server's roles from Discord.",
        )
        .await;
        return;
    };
    let Ok(roles) = roles_response.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back roles we couldn't read.",
        )
        .await;
        return;
    };

    let Ok(channels_response) = state.http.guild_channels(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server's channels from Discord.",
        )
        .await;
        return;
    };
    let Ok(channels) = channels_response.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back channels we couldn't read.",
        )
        .await;
        return;
    };

    let role_backups: Vec<RoleBackup> = roles
        .iter()
        // @everyone always exists and can't be recreated; `managed` roles
        // belong to an integration/bot and can't be manually recreated
        // the same way either.
        .filter(|role| role.id.get() != guild_id.get() && !role.managed)
        .map(|role| RoleBackup {
            name: role.name.clone(),
            permission_bits: role.permissions.bits(),
            color: role.color,
            hoist: role.hoist,
            mentionable: role.mentionable,
        })
        .collect();

    let channel_backups: Vec<ChannelBackup> = channels
        .iter()
        .filter(|channel| {
            matches!(
                channel.kind,
                ChannelType::GuildText | ChannelType::GuildCategory
            )
        })
        .filter_map(|channel| {
            channel.name.clone().map(|name| ChannelBackup {
                name,
                kind: channel.kind,
                topic: channel.topic.clone(),
            })
        })
        .collect();

    let backup = GuildBackup {
        guild_name: guild.name,
        roles: role_backups,
        channels: channel_backups,
    };

    let role_count = backup.roles.len();
    let channel_count = backup.channels.len();

    let backup_json = serde_json::to_string(&backup).expect("GuildBackup should always serialize");
    models::create_backup(&state.db, guild_id, &backup_json).await;

    respond(
        interaction,
        state,
        &format!("Backed up {role_count} role(s) and {channel_count} channel(s)."),
    )
    .await;
}

pub async fn handle_restore(interaction: &Interaction, state: &AppState) {
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
            "You need the **Manage Server** permission to run `/restore`.",
        )
        .await;
        return;
    }

    let Some(backup_json) = models::get_latest_backup_json(&state.db, guild_id).await else {
        respond(
            interaction,
            state,
            "No backup exists yet for this server — run `/backup` first.",
        )
        .await;
        return;
    };

    let backup: GuildBackup = match serde_json::from_str(&backup_json) {
        Ok(backup) => backup,
        Err(source) => {
            tracing::error!(?source, %guild_id, "failed to parse stored backup");
            respond(
                interaction,
                state,
                "The stored backup couldn't be read — please run `/backup` again.",
            )
            .await;
            return;
        }
    };

    let Ok(roles_response) = state.http.roles(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server's roles from Discord.",
        )
        .await;
        return;
    };
    let Ok(current_roles) = roles_response.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back roles we couldn't read.",
        )
        .await;
        return;
    };

    let Ok(channels_response) = state.http.guild_channels(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server's channels from Discord.",
        )
        .await;
        return;
    };
    let Ok(current_channels) = channels_response.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back channels we couldn't read.",
        )
        .await;
        return;
    };

    let mut roles_recreated = 0;
    for role in &backup.roles {
        let exists = current_roles
            .iter()
            .any(|current| current.name.eq_ignore_ascii_case(&role.name));

        if exists {
            continue;
        }

        let result = state
            .http
            .create_role(guild_id)
            .name(&role.name)
            .permissions(Permissions::from_bits_truncate(role.permission_bits))
            .color(role.color)
            .hoist(role.hoist)
            .mentionable(role.mentionable)
            .await;

        match result {
            Ok(_) => roles_recreated += 1,
            Err(source) => {
                tracing::error!(?source, %guild_id, role = %role.name, "failed to recreate role during /restore");
            }
        }
    }

    let mut channels_recreated = 0;
    for channel in &backup.channels {
        let exists = current_channels.iter().any(|current| {
            current
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&channel.name))
        });

        if exists {
            continue;
        }

        let mut request = state
            .http
            .create_guild_channel(guild_id, &channel.name)
            .kind(channel.kind);

        if let Some(topic) = channel.topic.as_deref() {
            request = request.topic(topic);
        }

        match request.await {
            Ok(_) => channels_recreated += 1,
            Err(source) => {
                tracing::error!(?source, %guild_id, channel = %channel.name, "failed to recreate channel during /restore");
            }
        }
    }

    respond(
        interaction,
        state,
        &format!(
            "Restore complete. Recreated {roles_recreated} role(s) and {channels_recreated} \
            channel(s) that were missing.\n\n\
            Note: recreated roles/channels get **new** Discord IDs — anything that referenced \
            the old ones (other bots' saved config, permission overwrites) will need manual \
            reattachment. This is a Discord limitation, not something Blackwall can work around."
        ),
    )
    .await;
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
        tracing::error!(
            ?source,
            "failed to respond to /backup or /restore interaction"
        );
    }
}
