use twilight_http::Client as HttpClient;
use twilight_model::{
    application::command::{Command, CommandType},
    id::{
        Id,
        marker::{ApplicationMarker, GuildMarker},
    },
};
use twilight_util::builder::command::CommandBuilder;

use crate::discord::{backup, lockdown, security_score, setup, verify_panel};

/// Asks Discord which application this bot's token belongs to.
///
/// We need the application ID to register slash commands, and this is the
/// simplest way to get it — no need to copy it into an env var by hand.
pub async fn fetch_application_id(
    http: &HttpClient,
) -> Result<Id<ApplicationMarker>, twilight_http::Error> {
    let application = http
        .current_user_application()
        .await?
        .model()
        .await
        .expect("Discord sent an application response we couldn't parse");

    Ok(application.id)
}

/// All slash commands Blackwall currently supports.
///
/// `/ping` proves the whole pipeline works end to end (register -> Discord
/// shows it -> user runs it -> we reply). Every other command carries real
/// logic and defines itself in its own module. `/config` remains the one
/// not built yet.
fn build_commands() -> Vec<Command> {
    let mut commands = vec![
        CommandBuilder::new(
            "ping",
            "Check that Blackwall is online and responding",
            CommandType::ChatInput,
        )
        .build(),
        setup::command(),
        verify_panel::command(),
        security_score::command(),
    ];

    commands.extend(lockdown::commands());
    commands.extend(backup::commands());
    commands
}

/// Registers the commands from [`build_commands`] with Discord.
///
/// If `guild_id` is set, commands are registered to just that one guild,
/// which Discord updates almost instantly — ideal while developing.
/// Otherwise commands are registered globally, which can take up to an
/// hour to show up in every server, but is what you want for a real launch.
pub async fn register(
    http: &HttpClient,
    application_id: Id<ApplicationMarker>,
    test_guild_id: Option<Id<GuildMarker>>,
) -> Result<(), twilight_http::Error> {
    let commands = build_commands();
    let interaction_client = http.interaction(application_id);

    match test_guild_id {
        Some(guild_id) => {
            interaction_client
                .set_guild_commands(guild_id, &commands)
                .await?;
            tracing::info!(%guild_id, "registered slash commands to test guild");
        }
        None => {
            interaction_client.set_global_commands(&commands).await?;
            tracing::info!("registered global slash commands");
        }
    }

    Ok(())
}
