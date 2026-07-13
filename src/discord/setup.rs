use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::application::interaction::{Interaction, InteractionContextType};
use twilight_model::channel::message::component::{
    ActionRow, Button, ButtonStyle, SelectDefaultValue, SelectMenu, SelectMenuType,
};
use twilight_model::channel::message::{Component, Embed, MessageFlags};
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_model::id::{
    marker::{ChannelMarker, GuildMarker, RoleMarker},
    Id,
};
use twilight_util::builder::command::CommandBuilder;
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::discord::embeds;
use crate::moderation::permissions;
use crate::state::AppState;
use crate::storage::models::{self, GuildConfig};

const COMPONENT_PREFIX: &str = "setup:";
const CREATE_DEFAULTS_ID: &str = "setup:create_defaults";
const LOG_CHANNEL_SELECT_ID: &str = "setup:log_channel";
const VERIFIED_ROLE_SELECT_ID: &str = "setup:verified_role";
const QUARANTINE_ROLE_SELECT_ID: &str = "setup:quarantine_role";
const TRUSTED_ROLE_SELECT_ID: &str = "setup:trusted_role";
const QUICK_CHECK_ID: &str = "setup:quick_check";

struct SetupPanel {
    embed: Embed,
    components: Vec<Component>,
}

/// Builds the lightweight `/setup` launcher. Configuration itself happens in
/// the private panel it opens, not through slash-command options.
pub fn command() -> Command {
    CommandBuilder::new(
        "setup",
        "Open Blackwall's interactive server setup panel",
        CommandType::ChatInput,
    )
    .contexts([InteractionContextType::Guild])
    .build()
}

/// Whether a component belongs to the interactive setup panel.
pub fn handles_component(custom_id: &str) -> bool {
    custom_id.starts_with(COMPONENT_PREFIX)
}

/// Opens the private setup panel. This does no Discord REST work, so the
/// command can always acknowledge immediately.
pub async fn handle_command(interaction: &Interaction, state: &AppState) {
    let Some(guild_id) = interaction.guild_id else {
        respond_error(
            interaction,
            state,
            "This command can only be used in a server.",
        )
        .await;
        return;
    };

    if !invoker_has_manage_guild(interaction) {
        respond_error(
            interaction,
            state,
            "You need the **Manage Server** permission to open Blackwall setup.",
        )
        .await;
        return;
    }

    let panel = build_panel(state, guild_id, None, &[]).await;
    respond_with_panel(interaction, state, panel).await;
}

/// Handles a selection or button click from the setup panel.
pub async fn handle_component(
    interaction: &Interaction,
    component: &MessageComponentInteractionData,
    state: &AppState,
) {
    let Some(guild_id) = interaction.guild_id else {
        respond_error(
            interaction,
            state,
            "This setup control can only be used in a server.",
        )
        .await;
        return;
    };

    if !invoker_has_manage_guild(interaction) {
        respond_error(
            interaction,
            state,
            "You need the **Manage Server** permission to change Blackwall setup.",
        )
        .await;
        return;
    }

    match component.custom_id.as_str() {
        CREATE_DEFAULTS_ID => create_defaults(interaction, state, guild_id).await,
        LOG_CHANNEL_SELECT_ID => {
            let Some(channel_id) = selected_id::<ChannelMarker>(component) else {
                update_panel(
                    interaction,
                    state,
                    guild_id,
                    Some("No log channel was selected."),
                    &[],
                )
                .await;
                return;
            };

            if !ensure_initialized(interaction, state, guild_id).await {
                return;
            }

            match models::set_log_channel_id(&state.db, guild_id, channel_id).await {
                Ok(()) => {
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Log channel updated."),
                        &[],
                    )
                    .await;
                }
                Err(source) => {
                    tracing::error!(?source, %guild_id, "failed to save setup log channel");
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Blackwall could not save that log channel. Please try again."),
                        &[],
                    )
                    .await;
                }
            }
        }
        VERIFIED_ROLE_SELECT_ID => {
            let Some(role_id) = selected_id::<RoleMarker>(component) else {
                update_panel(
                    interaction,
                    state,
                    guild_id,
                    Some("No verified role was selected."),
                    &[],
                )
                .await;
                return;
            };

            if !ensure_initialized(interaction, state, guild_id).await {
                return;
            }

            match models::set_verified_role_id(&state.db, guild_id, role_id).await {
                Ok(()) => {
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Verified role updated."),
                        &[],
                    )
                    .await;
                }
                Err(source) => {
                    tracing::error!(?source, %guild_id, "failed to save setup verified role");
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Blackwall could not save that verified role. Please try again."),
                        &[],
                    )
                    .await;
                }
            }
        }
        QUARANTINE_ROLE_SELECT_ID => {
            let Some(role_id) = selected_id::<RoleMarker>(component) else {
                update_panel(
                    interaction,
                    state,
                    guild_id,
                    Some("No quarantine role was selected."),
                    &[],
                )
                .await;
                return;
            };

            if !ensure_initialized(interaction, state, guild_id).await {
                return;
            }

            match models::set_quarantine_role_id(&state.db, guild_id, role_id).await {
                Ok(()) => {
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Quarantine role updated."),
                        &[],
                    )
                    .await;
                }
                Err(source) => {
                    tracing::error!(?source, %guild_id, "failed to save setup quarantine role");
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Blackwall could not save that quarantine role. Please try again."),
                        &[],
                    )
                    .await;
                }
            }
        }
        TRUSTED_ROLE_SELECT_ID => {
            let Some(role_id) = selected_id::<RoleMarker>(component) else {
                update_panel(
                    interaction,
                    state,
                    guild_id,
                    Some("No trusted role was selected."),
                    &[],
                )
                .await;
                return;
            };

            if !ensure_initialized(interaction, state, guild_id).await {
                return;
            }

            match models::set_trusted_role_id(&state.db, guild_id, role_id).await {
                Ok(()) => {
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Trusted role updated."),
                        &[],
                    )
                    .await;
                }
                Err(source) => {
                    tracing::error!(?source, %guild_id, "failed to save setup trusted role");
                    update_panel(
                        interaction,
                        state,
                        guild_id,
                        Some("Blackwall could not save that trusted role. Please try again."),
                        &[],
                    )
                    .await;
                }
            }
        }
        QUICK_CHECK_ID => run_quick_check(interaction, state, guild_id).await,
        _ => {
            tracing::warn!(custom_id = %component.custom_id, "received unknown setup component");
            update_panel(
                interaction,
                state,
                guild_id,
                Some("That setup control is no longer available. Open `/setup` again."),
                &[],
            )
            .await;
        }
    }
}

async fn create_defaults(interaction: &Interaction, state: &AppState, guild_id: Id<GuildMarker>) {
    if models::get_guild_config(&state.db, guild_id)
        .await
        .is_some()
    {
        update_panel(
            interaction,
            state,
            guild_id,
            Some("This server is already initialized. Use the selectors to make changes."),
            &[],
        )
        .await;
        return;
    }

    if !defer_update(interaction, state).await {
        return;
    }

    match initialize_defaults(state, guild_id).await {
        Ok(warnings) => {
            update_deferred_panel(
                interaction,
                state,
                guild_id,
                Some("Default log channel and security roles are ready."),
                &warnings,
            )
            .await;
        }
        Err(message) => {
            update_deferred_panel(interaction, state, guild_id, Some(&message), &[]).await;
        }
    }
}

async fn run_quick_check(interaction: &Interaction, state: &AppState, guild_id: Id<GuildMarker>) {
    if !defer_update(interaction, state).await {
        return;
    }

    match collect_quick_check(state, guild_id).await {
        Ok(warnings) => {
            let notice = if warnings.is_empty() {
                "Quick check complete. No high-signal permission risks found."
            } else {
                "Quick check complete. Review the findings below."
            };
            update_deferred_panel(interaction, state, guild_id, Some(notice), &warnings).await;
        }
        Err(message) => {
            update_deferred_panel(interaction, state, guild_id, Some(&message), &[]).await;
        }
    }
}

async fn ensure_initialized(
    interaction: &Interaction,
    state: &AppState,
    guild_id: Id<GuildMarker>,
) -> bool {
    if models::get_guild_config(&state.db, guild_id)
        .await
        .is_some()
    {
        true
    } else {
        update_panel(
            interaction,
            state,
            guild_id,
            Some("Create defaults before changing individual setup choices."),
            &[],
        )
        .await;
        false
    }
}

async fn initialize_defaults(
    state: &AppState,
    guild_id: Id<GuildMarker>,
) -> Result<Vec<String>, String> {
    let guild = state
        .http
        .guild(guild_id)
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to load guild during setup initialization");
            "Blackwall could not load this server from Discord. Please try again.".to_string()
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to decode guild during setup initialization");
            "Discord returned server details Blackwall could not read.".to_string()
        })?;
    let roles = state
        .http
        .roles(guild_id)
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to load roles during setup initialization");
            "Blackwall could not load this server's roles. Please try again.".to_string()
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to decode roles during setup initialization");
            "Discord returned roles Blackwall could not read.".to_string()
        })?;
    let channels = state
        .http
        .guild_channels(guild_id)
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to load channels during setup initialization");
            "Blackwall could not load this server's channels. Please try again.".to_string()
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to decode channels during setup initialization");
            "Discord returned channels Blackwall could not read.".to_string()
        })?;

    let verified_role_id = resolve_or_create_role(state, guild_id, &roles, "Verified").await?;
    let quarantine_role_id = resolve_or_create_role(state, guild_id, &roles, "Quarantine").await?;
    let log_channel_id = resolve_or_create_log_channel(state, guild_id, &channels).await?;

    let config = GuildConfig {
        guild_id,
        owner_id: guild.owner_id,
        log_channel_id: Some(log_channel_id),
        verified_role_id: Some(verified_role_id),
        quarantine_role_id: Some(quarantine_role_id),
        trusted_role_id: None,
    };
    models::upsert_guild_config(&state.db, &config)
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to save initialized guild config");
            "Blackwall created the defaults but could not save the setup. Please try again."
                .to_string()
        })?;

    let findings = permissions::check(&state.http, guild_id, &guild, &roles).await;
    models::upsert_security_score(
        &state.db,
        guild_id,
        findings.score(),
        &findings.critical,
        &findings.medium,
    )
    .await;

    Ok(findings.flattened())
}

async fn collect_quick_check(
    state: &AppState,
    guild_id: Id<GuildMarker>,
) -> Result<Vec<String>, String> {
    let guild = state
        .http
        .guild(guild_id)
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to load guild during setup quick check");
            "Blackwall could not load this server from Discord. Please try again.".to_string()
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to decode guild during setup quick check");
            "Discord returned server details Blackwall could not read.".to_string()
        })?;
    let roles = state
        .http
        .roles(guild_id)
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to load roles during setup quick check");
            "Blackwall could not load this server's roles. Please try again.".to_string()
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to decode roles during setup quick check");
            "Discord returned roles Blackwall could not read.".to_string()
        })?;

    let findings = permissions::check(&state.http, guild_id, &guild, &roles).await;
    models::upsert_security_score(
        &state.db,
        guild_id,
        findings.score(),
        &findings.critical,
        &findings.medium,
    )
    .await;

    Ok(findings.flattened())
}

async fn resolve_or_create_role(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    roles: &[twilight_model::guild::Role],
    wanted_name: &str,
) -> Result<Id<RoleMarker>, String> {
    if let Some(existing) = roles
        .iter()
        .find(|role| role.name.eq_ignore_ascii_case(wanted_name))
    {
        return Ok(existing.id);
    }

    let created = state
        .http
        .create_role(guild_id)
        .name(wanted_name)
        .permissions(Permissions::empty())
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, role = wanted_name, "failed to create role during setup initialization");
            format!("Blackwall could not create the {wanted_name} role. Check Manage Roles permission and role position.")
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, role = wanted_name, "failed to decode created role during setup initialization");
            format!("Discord returned an unreadable {wanted_name} role.")
        })?;

    tracing::info!(%guild_id, role = wanted_name, "created role during setup initialization");

    Ok(created.id)
}

async fn resolve_or_create_log_channel(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    channels: &[twilight_model::channel::Channel],
) -> Result<Id<ChannelMarker>, String> {
    if let Some(existing) = channels.iter().find(|channel| {
        channel.kind == ChannelType::GuildText
            && channel
                .name
                .as_deref()
                .is_some_and(|name| name.to_ascii_lowercase().contains("log"))
    }) {
        return Ok(existing.id);
    }

    let created = state
        .http
        .create_guild_channel(guild_id, "blackwall-logs")
        .kind(ChannelType::GuildText)
        .topic("Blackwall security logs")
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to create log channel during setup initialization");
            "Blackwall could not create #blackwall-logs. Check Manage Channels permission."
                .to_string()
        })?
        .model()
        .await
        .map_err(|source| {
            tracing::error!(?source, %guild_id, "failed to decode created log channel during setup initialization");
            "Discord returned an unreadable log channel.".to_string()
        })?;

    tracing::info!(%guild_id, "created #blackwall-logs during setup initialization");

    Ok(created.id)
}

async fn build_panel(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    notice: Option<&str>,
    warnings: &[String],
) -> SetupPanel {
    let config = models::get_guild_config(&state.db, guild_id).await;

    SetupPanel {
        embed: embeds::setup_panel(config.as_ref(), notice, warnings),
        components: build_components(config.as_ref()),
    }
}

fn build_components(config: Option<&GuildConfig>) -> Vec<Component> {
    let initialized = config.is_some();

    let log_channel_select = SelectMenu {
        channel_types: Some(vec![ChannelType::GuildText]),
        custom_id: LOG_CHANNEL_SELECT_ID.to_owned(),
        default_values: config
            .and_then(|config| config.log_channel_id)
            .map(|channel_id| vec![SelectDefaultValue::Channel(channel_id)]),
        disabled: !initialized,
        kind: SelectMenuType::Channel,
        max_values: Some(1),
        min_values: Some(1),
        options: None,
        placeholder: Some("Select log channel".to_owned()),
    };

    let verified_role_select = SelectMenu {
        channel_types: None,
        custom_id: VERIFIED_ROLE_SELECT_ID.to_owned(),
        default_values: config
            .and_then(|config| config.verified_role_id)
            .map(|role_id| vec![SelectDefaultValue::Role(role_id)]),
        disabled: !initialized,
        kind: SelectMenuType::Role,
        max_values: Some(1),
        min_values: Some(1),
        options: None,
        placeholder: Some("Select verified role".to_owned()),
    };

    let quarantine_role_select = SelectMenu {
        channel_types: None,
        custom_id: QUARANTINE_ROLE_SELECT_ID.to_owned(),
        default_values: config
            .and_then(|config| config.quarantine_role_id)
            .map(|role_id| vec![SelectDefaultValue::Role(role_id)]),
        disabled: !initialized,
        kind: SelectMenuType::Role,
        max_values: Some(1),
        min_values: Some(1),
        options: None,
        placeholder: Some("Select quarantine role".to_owned()),
    };

    let trusted_role_select = SelectMenu {
        channel_types: None,
        custom_id: TRUSTED_ROLE_SELECT_ID.to_owned(),
        default_values: config
            .and_then(|config| config.trusted_role_id)
            .map(|role_id| vec![SelectDefaultValue::Role(role_id)]),
        disabled: !initialized,
        kind: SelectMenuType::Role,
        max_values: Some(1),
        min_values: Some(1),
        options: None,
        placeholder: Some("Select trusted role (bot adds, invite links)".to_owned()),
    };

    let defaults_button = Button {
        custom_id: Some(CREATE_DEFAULTS_ID.to_owned()),
        disabled: initialized,
        emoji: None,
        label: Some(if initialized {
            "Defaults Ready".to_owned()
        } else {
            "Create Defaults".to_owned()
        }),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    };
    let quick_check_button = Button {
        custom_id: Some(QUICK_CHECK_ID.to_owned()),
        disabled: false,
        emoji: None,
        label: Some("Quick Check".to_owned()),
        style: ButtonStyle::Secondary,
        url: None,
        sku_id: None,
    };

    vec![
        Component::ActionRow(ActionRow {
            components: vec![Component::SelectMenu(log_channel_select)],
        }),
        Component::ActionRow(ActionRow {
            components: vec![Component::SelectMenu(verified_role_select)],
        }),
        Component::ActionRow(ActionRow {
            components: vec![Component::SelectMenu(quarantine_role_select)],
        }),
        Component::ActionRow(ActionRow {
            components: vec![Component::SelectMenu(trusted_role_select)],
        }),
        Component::ActionRow(ActionRow {
            components: vec![
                Component::Button(defaults_button),
                Component::Button(quick_check_button),
            ],
        }),
    ]
}

fn selected_id<T>(component: &MessageComponentInteractionData) -> Option<Id<T>> {
    component.values.first()?.parse().ok()
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

async fn respond_with_panel(interaction: &Interaction, state: &AppState, panel: SetupPanel) {
    let data = InteractionResponseDataBuilder::new()
        .embeds([panel.embed])
        .components(panel.components)
        .flags(MessageFlags::EPHEMERAL)
        .build();
    send_response(
        interaction,
        state,
        InteractionResponseType::ChannelMessageWithSource,
        Some(data),
    )
    .await;
}

async fn update_panel(
    interaction: &Interaction,
    state: &AppState,
    guild_id: Id<GuildMarker>,
    notice: Option<&str>,
    warnings: &[String],
) {
    let panel = build_panel(state, guild_id, notice, warnings).await;
    let data = InteractionResponseDataBuilder::new()
        .embeds([panel.embed])
        .components(panel.components)
        .build();
    send_response(
        interaction,
        state,
        InteractionResponseType::UpdateMessage,
        Some(data),
    )
    .await;
}

async fn defer_update(interaction: &Interaction, state: &AppState) -> bool {
    let response = InteractionResponse {
        kind: InteractionResponseType::DeferredUpdateMessage,
        data: None,
    };

    match state
        .http
        .interaction(state.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await
    {
        Ok(_) => true,
        Err(source) => {
            tracing::error!(?source, "failed to defer setup panel update");
            false
        }
    }
}

async fn update_deferred_panel(
    interaction: &Interaction,
    state: &AppState,
    guild_id: Id<GuildMarker>,
    notice: Option<&str>,
    warnings: &[String],
) {
    let panel = build_panel(state, guild_id, notice, warnings).await;
    let embeds = [panel.embed];

    if let Err(source) = state
        .http
        .interaction(state.application_id)
        .update_response(&interaction.token)
        .embeds(Some(&embeds))
        .components(Some(&panel.components))
        .await
    {
        tracing::error!(?source, "failed to update deferred setup panel");
    }
}

async fn respond_error(interaction: &Interaction, state: &AppState, content: &str) {
    let data = InteractionResponseDataBuilder::new()
        .content(content)
        .flags(MessageFlags::EPHEMERAL)
        .build();
    send_response(
        interaction,
        state,
        InteractionResponseType::ChannelMessageWithSource,
        Some(data),
    )
    .await;
}

async fn send_response(
    interaction: &Interaction,
    state: &AppState,
    kind: InteractionResponseType,
    data: Option<twilight_model::http::interaction::InteractionResponseData>,
) {
    let response = InteractionResponse { kind, data };

    if let Err(source) = state
        .http
        .interaction(state.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await
    {
        tracing::error!(?source, "failed to respond to setup interaction");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select_is_disabled(components: &[Component], row_index: usize) -> bool {
        let Component::ActionRow(row) = &components[row_index] else {
            panic!("expected an action row");
        };
        let Component::SelectMenu(menu) = &row.components[0] else {
            panic!("expected a select menu");
        };

        menu.disabled
    }

    #[test]
    fn selectors_wait_for_default_setup() {
        let components = build_components(None);

        assert_eq!(components.len(), 5);
        assert!(select_is_disabled(&components, 0));
        assert!(select_is_disabled(&components, 1));
        assert!(select_is_disabled(&components, 2));
        assert!(select_is_disabled(&components, 3));
    }

    #[test]
    fn initialized_setup_enables_selectors() {
        let config = GuildConfig {
            guild_id: Id::new(1),
            owner_id: Id::new(2),
            log_channel_id: Some(Id::new(3)),
            verified_role_id: Some(Id::new(4)),
            quarantine_role_id: Some(Id::new(5)),
            trusted_role_id: Some(Id::new(6)),
        };
        let components = build_components(Some(&config));

        assert!(!select_is_disabled(&components, 0));
        assert!(!select_is_disabled(&components, 1));
        assert!(!select_is_disabled(&components, 2));
        assert!(!select_is_disabled(&components, 3));
    }
}
