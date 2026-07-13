use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{Interaction, InteractionContextType};
use twilight_model::channel::message::{Embed, MessageFlags};
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::command::{CommandBuilder, IntegerBuilder};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::state::AppState;
use crate::storage::models;

const OPT_RAID_BURST: &str = "raid-burst";
const OPT_RAID_SUSPICIOUS: &str = "raid-suspicious";
const OPT_NUKE_BURST: &str = "nuke-burst";
const OPT_SPAM_BURST: &str = "spam-burst";
const OPT_SPAM_REPEAT: &str = "spam-repeat";
const OPT_SPAM_MENTION: &str = "spam-mention";
const OPT_SPAM_TIMEOUT_MINUTES: &str = "spam-timeout-minutes";
const OPT_RAID_TIMEOUT_MINUTES: &str = "raid-timeout-minutes";

/// Every threshold `/config` can adjust, one integer option each — all
/// optional, so a moderator sets only the ones they're changing. Each
/// `(option name, GuildSettings field it maps to)` pair also drives
/// dispatch in `handle`, so the two stay in lockstep by construction
/// rather than by two separately-maintained lists.
pub fn command() -> Command {
    CommandBuilder::new(
        "config",
        "Adjust Blackwall's detection thresholds for this server",
        CommandType::ChatInput,
    )
    .contexts([InteractionContextType::Guild])
    .option(
        IntegerBuilder::new(OPT_RAID_BURST, "Joins within 60s that count as a raid burst")
            .min_value(2)
            .max_value(1000)
            .required(false),
    )
    .option(
        IntegerBuilder::new(
            OPT_RAID_SUSPICIOUS,
            "Suspicious joins within 60s that count as a raid",
        )
        .min_value(1)
        .max_value(1000)
        .required(false),
    )
    .option(
        IntegerBuilder::new(
            OPT_NUKE_BURST,
            "Dangerous admin actions within 30s that count as a nuke attempt",
        )
        .min_value(1)
        .max_value(100)
        .required(false),
    )
    .option(
        IntegerBuilder::new(OPT_SPAM_BURST, "Messages within 10s that count as a burst")
            .min_value(2)
            .max_value(1000)
            .required(false),
    )
    .option(
        IntegerBuilder::new(
            OPT_SPAM_REPEAT,
            "Identical messages in a row that count as copy-paste spam",
        )
        .min_value(2)
        .max_value(1000)
        .required(false),
    )
    .option(
        IntegerBuilder::new(
            OPT_SPAM_MENTION,
            "Mentions in a single message that count as mention spam",
        )
        .min_value(1)
        .max_value(1000)
        .required(false),
    )
    .option(
        IntegerBuilder::new(OPT_SPAM_TIMEOUT_MINUTES, "Spam timeout duration, in minutes")
            .min_value(1)
            .max_value(40320)
            .required(false),
    )
    .option(
        IntegerBuilder::new(OPT_RAID_TIMEOUT_MINUTES, "Raid timeout duration, in minutes")
            .min_value(1)
            .max_value(40320)
            .required(false),
    )
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
            "You need the **Manage Server** permission to change Blackwall's configuration.",
        )
        .await;
        return;
    }

    let Some(twilight_model::application::interaction::InteractionData::ApplicationCommand(
        command,
    )) = interaction.data.as_ref()
    else {
        return;
    };

    let mut applied = Vec::new();

    for option in &command.options {
        let CommandOptionValue::Integer(value) = option.value else {
            continue;
        };
        let value = value as i32;

        let result = match option.name.as_str() {
            OPT_RAID_BURST => models::set_raid_burst_threshold(&state.db, guild_id, value).await,
            OPT_RAID_SUSPICIOUS => {
                models::set_raid_suspicious_threshold(&state.db, guild_id, value).await
            }
            OPT_NUKE_BURST => models::set_nuke_burst_threshold(&state.db, guild_id, value).await,
            OPT_SPAM_BURST => models::set_spam_burst_threshold(&state.db, guild_id, value).await,
            OPT_SPAM_REPEAT => models::set_spam_repeat_threshold(&state.db, guild_id, value).await,
            OPT_SPAM_MENTION => {
                models::set_spam_mention_threshold(&state.db, guild_id, value).await
            }
            OPT_SPAM_TIMEOUT_MINUTES => {
                models::set_spam_timeout_minutes(&state.db, guild_id, value).await
            }
            OPT_RAID_TIMEOUT_MINUTES => {
                models::set_raid_timeout_minutes(&state.db, guild_id, value).await
            }
            other => {
                tracing::warn!(option = other, "received unknown /config option");
                continue;
            }
        };

        match result {
            Ok(()) => applied.push(option.name.clone()),
            Err(source) => {
                tracing::error!(?source, %guild_id, option = %option.name, "failed to save /config threshold");
                respond(
                    interaction,
                    state,
                    "Blackwall could not save one of those settings. Please try again.",
                )
                .await;
                return;
            }
        }
    }

    if applied.is_empty() {
        respond(
            interaction,
            state,
            "No settings were changed — pass at least one option, e.g. `/config raid-burst:15`.",
        )
        .await;
        return;
    }

    // Every write above went straight to Postgres, bypassing the cache
    // entirely (`SettingsCache::get` is never on this path) — this only
    // needs to drop the stale in-memory copy so the next real event reads
    // fresh values, not to invalidate per-field.
    state.settings_cache.invalidate(guild_id);

    let settings = models::get_guild_settings(&state.db, guild_id).await;
    let embed = summary_embed(&settings, &applied);
    respond_with_embed(interaction, state, embed).await;
}

fn summary_embed(settings: &models::GuildSettings, applied: &[String]) -> Embed {
    EmbedBuilder::new()
        .title("Blackwall configuration updated")
        .color(0x2E_C4_6B)
        .field(EmbedFieldBuilder::new("Changed", applied.join(", ")))
        .field(
            EmbedFieldBuilder::new(
                "Anti-raid",
                format!(
                    "Burst: {} joins/60s\nSuspicious: {} joins/60s\nTimeout: {} minute(s)",
                    settings.raid_burst_threshold,
                    settings.raid_suspicious_threshold,
                    settings.raid_timeout_minutes
                ),
            )
            .inline(),
        )
        .field(
            EmbedFieldBuilder::new(
                "Anti-spam",
                format!(
                    "Burst: {} msgs/10s\nRepeat: {}\nMentions: {}\nTimeout: {} minute(s)",
                    settings.spam_burst_threshold,
                    settings.spam_repeat_threshold,
                    settings.spam_mention_threshold,
                    settings.spam_timeout_minutes
                ),
            )
            .inline(),
        )
        .field(
            EmbedFieldBuilder::new(
                "Anti-nuke",
                format!("Burst: {} actions/30s", settings.nuke_burst_threshold),
            )
            .inline(),
        )
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
    let data = InteractionResponseDataBuilder::new()
        .content(content)
        .flags(MessageFlags::EPHEMERAL)
        .build();
    send_response(interaction, state, data).await;
}

async fn respond_with_embed(interaction: &Interaction, state: &AppState, embed: Embed) {
    let data = InteractionResponseDataBuilder::new()
        .embeds([embed])
        .flags(MessageFlags::EPHEMERAL)
        .build();
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

    if let Err(source) = state
        .http
        .interaction(state.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await
    {
        tracing::error!(?source, "failed to respond to /config interaction");
    }
}
