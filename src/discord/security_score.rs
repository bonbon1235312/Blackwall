use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionContextType};
use twilight_model::channel::message::{Embed, MessageFlags};
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::command::CommandBuilder;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::moderation::permissions::{self, PermissionFindings};
use crate::state::AppState;
use crate::storage::models;

pub fn command() -> Command {
    CommandBuilder::new(
        "security-score",
        "Show this server's security score and the risks behind it",
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
            "You need the **Manage Server** permission to run `/security-score`.",
        )
        .await;
        return;
    }

    let Ok(guild) = state.http.guild(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server from Discord.",
        )
        .await;
        return;
    };
    let Ok(guild) = guild.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back a server we couldn't read.",
        )
        .await;
        return;
    };

    let Ok(roles) = state.http.roles(guild_id).await else {
        respond(
            interaction,
            state,
            "Couldn't load this server's roles from Discord.",
        )
        .await;
        return;
    };
    let Ok(roles) = roles.model().await else {
        respond(
            interaction,
            state,
            "Discord sent back roles we couldn't read.",
        )
        .await;
        return;
    };

    let mut findings = permissions::check(&state.http, guild_id, &guild, &roles).await;

    // Is the bot's own role positioned above every role that would need
    // moderating (anything with a dangerous permission)? If not, the
    // anti-nuke/quarantine responses that rely on the bot outranking the
    // target can't actually work.
    if let Ok(bot_member_response) = state.http.current_user_guild_member(guild_id).await {
        if let Ok(bot_member) = bot_member_response.model().await {
            let bot_highest_position = roles
                .iter()
                .filter(|role| bot_member.roles.contains(&role.id))
                .map(|role| role.position)
                .max()
                .unwrap_or(0);

            let dangerous_ids = permissions::dangerous_role_ids(&roles);
            let highest_dangerous_position = roles
                .iter()
                .filter(|role| dangerous_ids.contains(&role.id) && role.id.get() != guild_id.get())
                .map(|role| role.position)
                .max();

            if highest_dangerous_position.is_some_and(|position| bot_highest_position <= position) {
                findings.critical.push(
                    "Blackwall's own role is not positioned above other privileged roles in \
                Server Settings -> Roles — it may not be able to moderate members who have \
                them (anti-nuke role stripping, quarantine, timeouts)."
                        .to_string(),
                );
            }
        }
    }

    models::upsert_security_score(
        &state.db,
        guild_id,
        findings.score(),
        &findings.critical,
        &findings.medium,
    )
    .await;

    let embed = build_score_embed(&findings);
    respond_with_embed(interaction, state, embed).await;
}

fn build_score_embed(findings: &PermissionFindings) -> Embed {
    let score = findings.score();

    let critical_text = if findings.critical.is_empty() {
        "None found.".to_string()
    } else {
        findings
            .critical
            .iter()
            .map(|finding| format!("- {finding}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let medium_text = if findings.medium.is_empty() {
        "None found.".to_string()
    } else {
        findings
            .medium
            .iter()
            .map(|finding| format!("- {finding}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    EmbedBuilder::new()
        .title(format!("Security score: {score}/100"))
        .description(
            "This checks a handful of high-signal permission risks — not a full audit of \
            every setting. Anti-spam/scam/raid/nuke detection running is separate from this \
            score and doesn't affect it.",
        )
        .field(EmbedFieldBuilder::new("Critical risks", critical_text))
        .field(EmbedFieldBuilder::new("Medium risks", medium_text))
        .build()
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

    send_response(interaction, state, data).await;
}

async fn respond_with_embed(interaction: &Interaction, state: &AppState, embed: Embed) {
    let mut data = InteractionResponseDataBuilder::new()
        .embeds([embed])
        .build();
    data.flags = Some(MessageFlags::EPHEMERAL);

    send_response(interaction, state, data).await;
}

async fn send_response(
    interaction: &Interaction,
    state: &AppState,
    data: twilight_model::http::interaction::InteractionResponseData,
) {
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
        tracing::error!(?source, "failed to respond to /security-score interaction");
    }
}
