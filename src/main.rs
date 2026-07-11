mod actions;
mod config;
mod discord;
mod gateway;
mod moderation;
mod state;
mod storage;
mod utils;
mod verification;
mod web;

use std::sync::Arc;
use std::time::Duration;

use tracing_subscriber::EnvFilter;

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() {
    // Every TLS connection the bot makes (the gateway websocket, any HTTPS
    // request) goes through rustls, and rustls needs to be told up front
    // which crypto backend to use — it won't guess. This must happen
    // before anything below opens a connection.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls's crypto provider (should only fail if called twice)");

    // Sets up terminal logging. Override the default verbosity with e.g.
    // `RUST_LOG=blackwall=debug` if you need more detail while debugging.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::load();
    let http = discord::http::build_client(config.discord_token.clone());
    let oauth_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build OAuth HTTP client");
    let db = storage::database::connect(&config.database_path).await;

    let application_id = discord::commands::fetch_application_id(&http)
        .await
        .expect("failed to look up this bot's application ID from Discord");

    discord::commands::register(&http, application_id, config.test_guild_id)
        .await
        .expect("failed to register slash commands with Discord");

    let state = Arc::new(AppState {
        http,
        application_id,
        scam_matcher: moderation::scam::build_matcher(),
        db,
        spam_tracker: moderation::spam::SpamTracker::new(),
        join_tracker: moderation::raid::JoinTracker::new(),
        nuke_tracker: moderation::nuke::NukeTracker::new(),
        evasion_tracker: moderation::evasion::EvasionTracker::new(),
        oauth_client,
        sessions: verification::sessions::SessionStore::default(),
        dashboard_sessions: verification::dashboard::DashboardSessionStore::default(),
        discord_client_secret: config.discord_client_secret.clone(),
        public_base_url: config.public_base_url.clone(),
        support_guild_id: config.support_guild_id,
        support_server_invite_url: config.support_server_invite_url.clone(),
    });

    if config.discord_client_secret.is_some() {
        tokio::spawn(web::run(Arc::clone(&state), config.web_bind_addr.clone()));
    } else {
        tracing::warn!("verification web server disabled because DISCORD_CLIENT_SECRET is not set");
    }

    gateway::run(config.discord_token, state).await;
}
