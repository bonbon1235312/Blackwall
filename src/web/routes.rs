use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Router, routing::get};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::discord::embeds;
use crate::moderation::permissions;
use crate::state::AppState;
use crate::storage::models;
use crate::verification::{oauth, roles};
use crate::web::templates;

/// Name of the cookie holding an owner dashboard session token. Set
/// `HttpOnly` (never readable from page JavaScript) and `SameSite=Lax`
/// (sent on normal navigation, not on cross-site requests) — this project
/// doesn't add a cookie-handling dependency for this, since reading and
/// writing one cookie by hand is a handful of lines (see
/// `dashboard_session_user` and `set_session_cookie` below).
const DASHBOARD_COOKIE_NAME: &str = "bw_dashboard";

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(landing))
        .route("/verify", get(verify))
        .route("/callback", get(callback))
        .route("/success", get(success))
        .route("/privacy", get(privacy))
        .route("/terms", get(terms))
        .route("/support", get(support))
        .route("/dashboard/login", get(dashboard_login))
        .route("/dashboard/callback", get(dashboard_callback))
        .route("/dashboard", get(dashboard_home))
        .route("/dashboard/{guild_id}", get(dashboard_guild))
        .with_state(state)
}

/// Whether a server has both opted into support-server join (`/setup
/// support_server_join:true`) and this Blackwall instance actually has a
/// support server configured (`SUPPORT_GUILD_ID`). Both need to be true —
/// checked here, once, so `/verify` (deciding what to disclose and which
/// OAuth scope to request) and `/callback` (deciding whether to actually
/// attempt the join) can never disagree with each other.
async fn support_join_offered(state: &AppState, guild_id: Id<GuildMarker>) -> bool {
    state.support_guild_id.is_some()
        && models::get_guild_settings(&state.db, guild_id)
            .await
            .support_join_enabled
}

/// Disclosed on the verify page (in visible body text, not fine print)
/// whenever a server has opted its members into the support-server-join
/// feature. Kept as one constant so the Discord-side warning
/// (`/setup`'s "Support-server join" summary field would be a poor place
/// for legal-ish copy) and the actual consent page always say the same
/// thing.
const SUPPORT_JOIN_DISCLOSURE: &str = "By continuing, you authorise this app to verify your \
    Discord account. This may also add you to our official support/community server so you can \
    receive support, updates, and security alerts.";

async fn landing() -> Html<String> {
    Html(templates::landing_page())
}

async fn success(Query(query): Query<HashMap<String, String>>) -> Html<String> {
    let support_joined = query.get("support_joined").map(|value| value == "true");

    Html(templates::success_page(support_joined))
}

async fn privacy() -> Html<String> {
    Html(templates::privacy_page())
}

async fn terms() -> Html<String> {
    Html(templates::terms_page())
}

async fn support(State(state): State<Arc<AppState>>) -> Html<String> {
    Html(templates::support_page(
        state.support_server_invite_url.as_deref(),
    ))
}

async fn verify(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Html<String> {
    let Some(guild_id) = parse_guild_id(&query) else {
        return Html(templates::error_page(
            "This verification link is missing a valid server ID.",
        ));
    };

    if state.discord_client_secret.is_none() {
        return Html(templates::error_page(
            "Verification is not configured for this Blackwall instance yet.",
        ));
    }

    let guild_name = match state.http.guild(guild_id).await {
        Ok(response) => match response.model().await {
            Ok(guild) => guild.name,
            Err(source) => {
                tracing::warn!(?source, %guild_id, "failed to decode guild while rendering verify page");
                return Html(templates::error_page(
                    "Discord sent back server details Blackwall could not read.",
                ));
            }
        },
        Err(source) => {
            tracing::warn!(?source, %guild_id, "failed to load guild while rendering verify page");
            return Html(templates::error_page(
                "Blackwall could not load this server from Discord. Make sure the bot is still in the server.",
            ));
        }
    };

    let offer_support_join = support_join_offered(&state, guild_id).await;

    let state_token = state.sessions.create(guild_id);
    let redirect_uri = redirect_uri(&state);
    let oauth_url = oauth::authorize_url(
        state.application_id,
        &redirect_uri,
        &state_token,
        offer_support_join,
    );

    let disclosure = offer_support_join.then_some(SUPPORT_JOIN_DISCLOSURE);

    Html(templates::verify_page(&guild_name, &oauth_url, disclosure))
}

async fn callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(client_secret) = state.discord_client_secret.as_deref() else {
        return Html(templates::error_page(
            "Verification is not configured for this Blackwall instance yet.",
        ))
        .into_response();
    };

    let Some(code) = non_empty_query_value(&query, "code") else {
        return Html(templates::error_page(
            "Discord did not return an authorization code.",
        ))
        .into_response();
    };

    let Some(state_token) = non_empty_query_value(&query, "state") else {
        return Html(templates::error_page(
            "Discord did not return a verification state token.",
        ))
        .into_response();
    };

    let Some(pending) = state.sessions.take(&state_token) else {
        return Html(templates::error_page(
            "This verification link expired or was already used. Go back to Discord and click Verify again.",
        ))
        .into_response();
    };

    let redirect_uri = redirect_uri(&state);
    let token = match oauth::exchange_code(
        &state.oauth_client,
        state.application_id,
        client_secret,
        &redirect_uri,
        &code,
    )
    .await
    {
        Ok(token) => token,
        Err(source) => {
            tracing::warn!(?source, guild_id = %pending.guild_id, "Discord OAuth token exchange failed");
            return Html(templates::error_page(
                "Discord did not authorize this verification request. Please try again.",
            ))
            .into_response();
        }
    };

    let user = match oauth::fetch_current_user(&state.oauth_client, &token.access_token).await {
        Ok(user) => user,
        Err(oauth::FetchCurrentUserError::Request(source)) => {
            tracing::warn!(?source, guild_id = %pending.guild_id, "failed to fetch Discord OAuth user");
            return Html(templates::error_page(
                "Blackwall could not read your Discord account after authorization.",
            ))
            .into_response();
        }
        Err(oauth::FetchCurrentUserError::InvalidUserId(raw_id)) => {
            tracing::warn!(%raw_id, guild_id = %pending.guild_id, "Discord returned an invalid user ID");
            return Html(templates::error_page(
                "Discord returned account details Blackwall could not read.",
            ))
            .into_response();
        }
    };

    if let Err(source) =
        roles::grant_verified_role(state.http.as_ref(), &state.db, pending.guild_id, user.id).await
    {
        return match source {
            roles::GrantVerifiedRoleError::GuildNotSetUp => Html(templates::error_page(
                "This server has not finished Blackwall setup yet. Ask an admin to run /setup first.",
            ))
            .into_response(),
            roles::GrantVerifiedRoleError::Discord(source) => {
                tracing::warn!(
                    ?source,
                    guild_id = %pending.guild_id,
                    user_id = %user.id,
                    "failed to grant verified role"
                );
                Html(templates::error_page(
                    "Blackwall could not grant the Verified role. Ask an admin to check the bot's role position and permissions.",
                ))
                .into_response()
            }
            roles::GrantVerifiedRoleError::Storage(source) => {
                tracing::error!(
                    ?source,
                    guild_id = %pending.guild_id,
                    user_id = %user.id,
                    "failed to record verification"
                );
                Html(templates::error_page(
                    "Blackwall verified your account, but could not save the verification record.",
                ))
                .into_response()
            }
        };
    }

    tracing::info!(
        guild_id = %pending.guild_id,
        user_id = %user.id,
        username = %user.username,
        "user completed OAuth verification"
    );

    if let Err(source) = models::record_security_event(
        &state.db,
        pending.guild_id,
        Some(user.id),
        "verification_success",
        "info",
        "User completed OAuth verification and received the Verified role.",
    )
    .await
    {
        tracing::warn!(?source, guild_id = %pending.guild_id, user_id = %user.id, "failed to record verification security event");
    }

    // Best-effort, and only attempted at all if the /verify page actually
    // disclosed and requested `guilds.join` for this session — never
    // block the primary verification result (already saved above) on
    // this succeeding or failing.
    let support_joined = if support_join_offered(&state, pending.guild_id).await {
        Some(attempt_support_join(&state, pending.guild_id, &user, &token.access_token).await)
    } else {
        None
    };

    match support_joined {
        Some(true) => Redirect::to("/success?support_joined=true").into_response(),
        Some(false) => Redirect::to("/success?support_joined=false").into_response(),
        None => Redirect::to("/success").into_response(),
    }
}

/// Attempts to add a freshly-verified user to the support server using
/// their OAuth access token (which must include the `guilds.join` scope
/// for this to succeed — guaranteed by only calling this when
/// `support_join_offered` was true for the same session). Logs and
/// records the outcome either way, per the "log support-server join
/// success/fail" requirement — but the caller treats both outcomes as
/// "verification still succeeded," since this is a bonus, not the point.
async fn attempt_support_join(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    user: &oauth::DiscordUser,
    access_token: &str,
) -> bool {
    let Some(support_guild_id) = state.support_guild_id else {
        return false;
    };

    let result = state
        .http
        .add_guild_member(support_guild_id, user.id, access_token)
        .await;

    let (succeeded, severity, description) = match result {
        Ok(_) => (
            true,
            "info",
            "User was added to (or already in) the official support server.".to_string(),
        ),
        Err(source) => {
            tracing::warn!(
                ?source,
                %guild_id,
                user_id = %user.id,
                "failed to add verified user to the support server"
            );
            (
                false,
                "low",
                format!("Could not add the user to the official support server: {source}"),
            )
        }
    };

    if let Err(source) = models::record_security_event(
        &state.db,
        guild_id,
        Some(user.id),
        if succeeded {
            "support_server_join_success"
        } else {
            "support_server_join_failed"
        },
        severity,
        &description,
    )
    .await
    {
        tracing::warn!(?source, %guild_id, user_id = %user.id, "failed to record support-join security event");
    }

    if let Some(log_channel_id) = models::get_log_channel_id(&state.db, guild_id).await {
        let embed = embeds::support_join_result(user, succeeded);

        if let Err(source) = state
            .http
            .create_message(log_channel_id)
            .embeds(&[embed])
            .await
        {
            tracing::error!(?source, %guild_id, "failed to send support-join log embed");
        }
    }

    succeeded
}

fn parse_guild_id(query: &HashMap<String, String>) -> Option<Id<GuildMarker>> {
    let raw = query.get("guild_id")?;
    let parsed = raw.parse::<u64>().ok()?;

    Some(Id::new(parsed))
}

fn non_empty_query_value(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query.get(key).filter(|value| !value.is_empty()).cloned()
}

fn redirect_uri(state: &AppState) -> String {
    format!("{}/callback", state.public_base_url)
}

fn dashboard_redirect_uri(state: &AppState) -> String {
    format!("{}/dashboard/callback", state.public_base_url)
}

/// Starts a dashboard login: creates a CSRF state token and sends the
/// owner straight to Discord. There's no separate "sign in" landing page —
/// a link to `/dashboard/login` (from `/dashboard` when no session cookie
/// is present, or from a nav link) is enough, same as any ordinary
/// "Sign in with Discord" button.
async fn dashboard_login(State(state): State<Arc<AppState>>) -> Response {
    if state.discord_client_secret.is_none() {
        return Html(templates::error_page(
            "The owner dashboard is not configured for this Blackwall instance yet.",
        ))
        .into_response();
    }

    let state_token = state.dashboard_sessions.create_login_state();
    let redirect_uri = dashboard_redirect_uri(&state);
    let oauth_url =
        oauth::dashboard_authorize_url(state.application_id, &redirect_uri, &state_token);

    Redirect::to(&oauth_url).into_response()
}

async fn dashboard_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(client_secret) = state.discord_client_secret.as_deref() else {
        return Html(templates::error_page(
            "The owner dashboard is not configured for this Blackwall instance yet.",
        ))
        .into_response();
    };

    let Some(code) = non_empty_query_value(&query, "code") else {
        return Html(templates::error_page(
            "Discord did not return an authorization code.",
        ))
        .into_response();
    };

    let Some(state_token) = non_empty_query_value(&query, "state") else {
        return Html(templates::error_page(
            "Discord did not return a login state token.",
        ))
        .into_response();
    };

    if !state.dashboard_sessions.consume_login_state(&state_token) {
        return Html(templates::error_page(
            "This dashboard login link expired or was already used. Go back to Discord and sign in again.",
        ))
        .into_response();
    }

    let redirect_uri = dashboard_redirect_uri(&state);
    let token = match oauth::exchange_code(
        &state.oauth_client,
        state.application_id,
        client_secret,
        &redirect_uri,
        &code,
    )
    .await
    {
        Ok(token) => token,
        Err(source) => {
            tracing::warn!(?source, "dashboard OAuth token exchange failed");
            return Html(templates::error_page(
                "Discord did not authorize this dashboard login. Please try again.",
            ))
            .into_response();
        }
    };

    let user = match oauth::fetch_current_user(&state.oauth_client, &token.access_token).await {
        Ok(user) => user,
        Err(source) => {
            tracing::warn!(
                ?source,
                "failed to fetch Discord OAuth user for dashboard login"
            );
            return Html(templates::error_page(
                "Blackwall could not read your Discord account after authorization.",
            ))
            .into_response();
        }
    };

    let session_token = state.dashboard_sessions.create_session(user.id);
    let mut response = Redirect::to("/dashboard").into_response();
    set_session_cookie(
        response.headers_mut(),
        &session_token,
        state.public_base_url.starts_with("https://"),
    );

    response
}

async fn dashboard_home(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(user_id) = dashboard_session_user(&state, &headers) else {
        return Redirect::to("/dashboard/login").into_response();
    };

    let guild_ids = models::get_guilds_owned_by(&state.db, user_id).await;
    let mut guilds = Vec::with_capacity(guild_ids.len());

    for guild_id in guild_ids {
        let name = match state.http.guild(guild_id).await {
            Ok(response) => match response.model().await {
                Ok(guild) => guild.name,
                Err(_) => guild_id.to_string(),
            },
            Err(_) => guild_id.to_string(),
        };

        guilds.push((guild_id, name));
    }

    Html(templates::dashboard_list_page(&guilds)).into_response()
}

async fn dashboard_guild(
    State(state): State<Arc<AppState>>,
    Path(guild_id_raw): Path<u64>,
    headers: HeaderMap,
) -> Response {
    let Some(user_id) = dashboard_session_user(&state, &headers) else {
        return Redirect::to("/dashboard/login").into_response();
    };

    let guild_id = Id::new(guild_id_raw);

    // The whole of this dashboard's access control: the logged-in Discord
    // user must match the `owner_id` Blackwall recorded for this guild
    // during `/setup`. No separate ACL table, no re-asking Discord.
    if models::get_owner_id(&state.db, guild_id).await != Some(user_id) {
        return Html(templates::error_page(
            "You don't have access to this server's dashboard.",
        ))
        .into_response();
    }

    let guild = match state.http.guild(guild_id).await {
        Ok(response) => match response.model().await {
            Ok(guild) => guild,
            Err(source) => {
                tracing::warn!(?source, %guild_id, "failed to decode guild while rendering dashboard");
                return Html(templates::error_page(
                    "Discord sent back server details Blackwall could not read.",
                ))
                .into_response();
            }
        },
        Err(source) => {
            tracing::warn!(?source, %guild_id, "failed to load guild while rendering dashboard");
            return Html(templates::error_page(
                "Blackwall could not load this server from Discord. Make sure the bot is still in the server.",
            ))
            .into_response();
        }
    };

    let roles = match state.http.roles(guild_id).await {
        Ok(response) => match response.model().await {
            Ok(roles) => roles,
            Err(source) => {
                tracing::warn!(?source, %guild_id, "failed to decode roles while rendering dashboard");
                return Html(templates::error_page(
                    "Discord sent back role details Blackwall could not read.",
                ))
                .into_response();
            }
        },
        Err(source) => {
            tracing::warn!(?source, %guild_id, "failed to load roles while rendering dashboard");
            return Html(templates::error_page(
                "Blackwall could not load this server's roles from Discord.",
            ))
            .into_response();
        }
    };

    let findings = permissions::check(&state.http, guild_id, &guild, &roles).await;
    let events = models::get_recent_security_events(&state.db, guild_id, 10).await;

    Html(templates::dashboard_guild_page(
        &guild.name,
        &findings,
        &events,
    ))
    .into_response()
}

/// Reads and validates the dashboard session cookie, if present.
fn dashboard_session_user(state: &AppState, headers: &HeaderMap) -> Option<Id<UserMarker>> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    let token = read_cookie(cookie_header, DASHBOARD_COOKIE_NAME)?;

    state.dashboard_sessions.user_id(token)
}

fn read_cookie<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    cookie_header.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn set_session_cookie(headers: &mut HeaderMap, token: &str, secure: bool) {
    let secure_attr = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{DASHBOARD_COOKIE_NAME}={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age=86400{secure_attr}"
    );

    if let Ok(value) = HeaderValue::from_str(&cookie) {
        headers.insert(header::SET_COOKIE, value);
    } else {
        tracing::error!("failed to build dashboard session cookie header");
    }
}
