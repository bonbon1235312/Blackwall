use std::env;

use twilight_model::id::{Id, marker::GuildMarker};

/// Bot configuration, loaded once at startup from environment variables.
///
/// During development these can live in a `.env` file next to `Cargo.toml`
/// (see `.env.example`). In production you'd set real environment
/// variables instead — either way, `dotenvy::dotenv()` in [`Config::load`]
/// makes both cases work with the same code.
pub struct Config {
    /// The bot's secret token from the Discord Developer Portal.
    /// Never print or log this value.
    pub discord_token: String,

    /// If set, slash commands are registered to this one guild instead of
    /// globally. Guild-scoped commands update within seconds; global
    /// commands can take up to an hour to reach every server. Use this
    /// while developing, remove it (or leave it unset) for production.
    pub test_guild_id: Option<Id<GuildMarker>>,

    /// Path to the SQLite database file. Created automatically if it
    /// doesn't exist yet.
    pub database_path: String,

    /// Discord OAuth client secret. If absent, the verification website is
    /// disabled but the Discord bot still runs normally.
    pub discord_client_secret: Option<String>,

    /// Public URL users reach in their browser. This is used in OAuth
    /// redirect URLs and in the link posted by `/verify-panel`.
    pub public_base_url: String,

    /// Address the local web server binds to.
    pub web_bind_addr: String,

    /// The bot's own official support/community server. If unset, the
    /// support-server-join feature is unavailable everywhere, regardless of
    /// any per-guild `support_join_enabled` toggle.
    pub support_guild_id: Option<Id<GuildMarker>>,

    /// A plain, public Discord invite link (e.g. `https://discord.gg/...`)
    /// for the support server. Separate from `support_guild_id`: this one
    /// is just a link shown to humans (the `/support` page, the landing
    /// page's "Support Server" button) — it's not used for the OAuth
    /// auto-join API call, which only needs the guild ID.
    pub support_server_invite_url: Option<String>,
}

impl Config {
    /// Reads configuration from the environment, panicking with a clear
    /// message if anything required is missing or invalid.
    ///
    /// It's fine to panic here: without a valid token there is nothing
    /// useful the bot can do, so failing fast at startup is the right move.
    pub fn load() -> Self {
        // If there's no `.env` file this just does nothing, which is
        // expected in production — so we ignore the error.
        let _ = dotenvy::dotenv();

        let discord_token = env::var("DISCORD_TOKEN")
            .expect("DISCORD_TOKEN must be set (in the environment or a .env file)");

        let test_guild_id = non_empty_env("TEST_GUILD_ID").map(|raw| {
            let parsed: u64 = raw
                .parse()
                .expect("TEST_GUILD_ID must be a valid Discord guild ID (numbers only)");
            Id::new(parsed)
        });

        let database_path =
            non_empty_env("DATABASE_PATH").unwrap_or_else(|| "blackwall.db".to_string());

        let discord_client_secret = non_empty_env("DISCORD_CLIENT_SECRET");

        let public_base_url = non_empty_env("PUBLIC_BASE_URL")
            .unwrap_or_else(|| "http://localhost:8080".to_string())
            .trim_end_matches('/')
            .to_string();

        let web_bind_addr =
            non_empty_env("WEB_BIND_ADDR").unwrap_or_else(|| "127.0.0.1:8080".to_string());

        let support_guild_id = non_empty_env("SUPPORT_GUILD_ID").map(|raw| {
            let parsed: u64 = raw
                .parse()
                .expect("SUPPORT_GUILD_ID must be a valid Discord guild ID (numbers only)");
            Id::new(parsed)
        });

        let support_server_invite_url = non_empty_env("SUPPORT_SERVER_INVITE_URL");

        Config {
            discord_token,
            test_guild_id,
            database_path,
            discord_client_secret,
            public_base_url,
            web_bind_addr,
            support_guild_id,
            support_server_invite_url,
        }
    }
}

/// Reads an environment variable, treating both "unset" and "set to an
/// empty string" as absent.
///
/// This matters because a `.env` file with `SOME_KEY=` (no value after the
/// `=`) sets the variable to `""`, not "not present" — and that's exactly
/// what an optional field left blank in `.env.example` looks like. Without
/// this, an optional setting left blank would silently take the empty
/// string instead of falling back to its default.
fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}
