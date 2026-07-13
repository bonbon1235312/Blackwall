use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{InteractionContextType, InteractionData};
use twilight_model::channel::message::Embed;
use twilight_model::guild::Permissions;
use twilight_util::builder::command::{CommandBuilder, IntegerBuilder};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::discord::source::CommandSource;
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

/// Every recognized `key:value` pair, sourced from either the slash
/// command's real integer options or (for a prefix invocation) parsed
/// text like `raid-burst:15 nuke-burst:2`. Unrecognized keys and
/// unparseable values are silently skipped, same as an interaction option
/// that isn't one of the ones `command()` declares.
fn parse_args(source: &CommandSource<'_>, prefix_args: Option<&str>) -> Vec<(&'static str, i32)> {
    const KNOWN: [&str; 8] = [
        OPT_RAID_BURST,
        OPT_RAID_SUSPICIOUS,
        OPT_NUKE_BURST,
        OPT_SPAM_BURST,
        OPT_SPAM_REPEAT,
        OPT_SPAM_MENTION,
        OPT_SPAM_TIMEOUT_MINUTES,
        OPT_RAID_TIMEOUT_MINUTES,
    ];

    match source {
        CommandSource::Interaction(interaction) => {
            let Some(InteractionData::ApplicationCommand(command)) = interaction.data.as_ref()
            else {
                return Vec::new();
            };

            command
                .options
                .iter()
                .filter_map(|option| {
                    let CommandOptionValue::Integer(value) = option.value else {
                        return None;
                    };
                    let key = KNOWN.iter().find(|known| **known == option.name)?;
                    Some((*key, value as i32))
                })
                .collect()
        }
        CommandSource::Message(_) => {
            let Some(args) = prefix_args else {
                return Vec::new();
            };

            args.split_whitespace()
                .filter_map(|token| {
                    let (raw_key, raw_value) = token.split_once(':')?;
                    let key = KNOWN.iter().find(|known| **known == raw_key)?;
                    let value: i32 = raw_value.parse().ok()?;
                    Some((*key, value))
                })
                .collect()
        }
    }
}

pub async fn handle(source: &CommandSource<'_>, state: &AppState, prefix_args: Option<&str>) {
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
                "You need the **Manage Server** permission to change Blackwall's configuration.",
            )
            .await;
        return;
    }

    let mut applied = Vec::new();

    for (key, value) in parse_args(source, prefix_args) {
        let result = match key {
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
            Ok(()) => applied.push(key),
            Err(source_err) => {
                tracing::error!(?source_err, %guild_id, option = key, "failed to save /config threshold");
                source
                    .reply(
                        state,
                        "Blackwall could not save one of those settings. Please try again.",
                    )
                    .await;
                return;
            }
        }
    }

    if applied.is_empty() {
        source
            .reply(
                state,
                "No settings were changed — pass at least one option, e.g. `/config raid-burst:15` \
                or `!config raid-burst:15`.",
            )
            .await;
        return;
    }

    // Every write above went straight to Postgres, bypassing the cache
    // entirely (`SettingsCache::get` is never on this path) — this only
    // needs to drop the stale in-memory copy so the next real event reads
    // fresh values, not to invalidate per-field.
    state.settings_cache.invalidate(guild_id);

    // The full-values summary embed is fine for a slash command's private
    // reply, but a `!config` prefix confirmation is always a public
    // channel message — dumping every current threshold there (not just
    // what changed) would hand a would-be raider a precise readout of
    // exactly what to stay under. The write itself still happens either
    // way; only the reply's detail level differs.
    match source {
        CommandSource::Interaction(_) => {
            let settings = models::get_guild_settings(&state.db, guild_id).await;
            let embed = summary_embed(&settings, &applied);
            source.reply_with_embed(state, embed).await;
        }
        CommandSource::Message(_) => {
            source
                .reply(state, &format!("Updated: {}.", applied.join(", ")))
                .await;
        }
    }
}

fn summary_embed(settings: &models::GuildSettings, applied: &[&str]) -> Embed {
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
