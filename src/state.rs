use std::sync::Arc;

use aho_corasick::AhoCorasick;
use reqwest::Client as ReqwestClient;
use sqlx::SqlitePool;
use twilight_http::Client as HttpClient;
use twilight_model::id::{
    Id,
    marker::{ApplicationMarker, GuildMarker},
};

use crate::moderation::nuke::NukeTracker;
use crate::moderation::raid::JoinTracker;
use crate::moderation::spam::SpamTracker;
use crate::verification::dashboard::DashboardSessionStore;
use crate::verification::sessions::SessionStore;

/// State shared across the entire bot.
///
/// `main.rs` wraps this in an `Arc` (a reference-counted pointer) so every
/// async task — handling a message, handling a slash command, and so on —
/// can hold a cheap handle to the same data without copying it.
///
/// This will keep growing over the next stages (spam trackers, guild
/// settings cache, etc.).
pub struct AppState {
    pub http: Arc<HttpClient>,
    pub application_id: Id<ApplicationMarker>,

    /// Compiled once at startup (see `moderation::scam::build_matcher`) and
    /// reused for every message — rebuilding it per-message would throw
    /// away the whole point of using Aho-Corasick.
    pub scam_matcher: AhoCorasick,

    /// Per-guild configuration and settings (see `storage/`). `SqlitePool`
    /// is already cheap to clone/share internally, so it's stored directly
    /// rather than behind another `Arc`.
    pub db: SqlitePool,

    /// Recent per-user message activity, used to detect spam that spans
    /// multiple messages (bursts, copy-pasted repeats).
    pub spam_tracker: SpamTracker,

    /// HTTP client for OAuth calls that are not covered by twilight-http.
    pub oauth_client: ReqwestClient,

    /// One-time OAuth state tokens for member verification.
    pub sessions: SessionStore,

    /// Secret used to exchange Discord OAuth authorization codes. `None`
    /// means the verification website is disabled.
    pub discord_client_secret: Option<String>,

    /// Public base URL used to build verification links and OAuth redirects.
    pub public_base_url: String,

    /// The bot's own support/community server. `None` means the
    /// support-server-join feature is unavailable regardless of any
    /// per-guild `support_join_enabled` toggle.
    pub support_guild_id: Option<Id<GuildMarker>>,

    /// A plain public invite link for the support server, shown to humans
    /// (the `/support` page, the landing page). Not used for the OAuth
    /// auto-join API call.
    pub support_server_invite_url: Option<String>,

    /// Recent per-guild join activity, used to detect raids (join bursts,
    /// waves of suspicious new accounts).
    pub join_tracker: JoinTracker,

    /// Recent per-(guild, actor) dangerous audit-log actions, used to
    /// detect nuke attempts.
    pub nuke_tracker: NukeTracker,

    /// Owner dashboard logins — a separate, longer-lived mechanism from
    /// `sessions` above (see `verification::dashboard`'s own docs for why
    /// the two aren't the same store).
    pub dashboard_sessions: DashboardSessionStore,
}
