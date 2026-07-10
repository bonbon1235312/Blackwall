use serde::{Deserialize, Serialize};
use twilight_model::id::{
    Id,
    marker::{ApplicationMarker, UserMarker},
};

pub fn authorize_url(
    client_id: Id<ApplicationMarker>,
    redirect_uri: &str,
    state_token: &str,
    include_guilds_join: bool,
) -> String {
    let mut url = reqwest::Url::parse("https://discord.com/api/oauth2/authorize")
        .expect("Discord's OAuth URL should be valid");
    let scope = if include_guilds_join {
        "identify guilds.join"
    } else {
        "identify"
    };

    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &client_id.to_string())
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", scope)
            .append_pair("state", state_token);
    }

    url.to_string()
}

/// Builds the OAuth authorize URL for the *owner dashboard* login flow —
/// deliberately a separate function from [`authorize_url`], not the same
/// function with another boolean flag, even though it currently produces
/// the same `identify`-only scope. Member verification and dashboard
/// login are different purposes with different redirect URIs and — if
/// the dashboard ever needs more than `identify` — different scopes;
/// conflating them in one function risks the two flows drifting into each
/// other by accident. (This doesn't need the `guilds` scope: dashboard
/// access is entirely determined by cross-referencing the logged-in
/// user's ID against `guilds.owner_id` in Blackwall's own database, since
/// that's already recorded by `/setup` — no need to ask Discord which
/// servers the user is in.)
pub fn dashboard_authorize_url(
    client_id: Id<ApplicationMarker>,
    redirect_uri: &str,
    state_token: &str,
) -> String {
    let mut url = reqwest::Url::parse("https://discord.com/api/oauth2/authorize")
        .expect("Discord's OAuth URL should be valid");

    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &client_id.to_string())
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", "identify")
            .append_pair("state", state_token);
    }

    url.to_string()
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
}

#[derive(Serialize)]
struct TokenRequest<'a> {
    grant_type: &'static str,
    code: &'a str,
    redirect_uri: &'a str,
    client_id: String,
    client_secret: &'a str,
}

pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: Id<ApplicationMarker>,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
) -> Result<TokenResponse, reqwest::Error> {
    let request = TokenRequest {
        grant_type: "authorization_code",
        code,
        redirect_uri,
        client_id: client_id.to_string(),
        client_secret,
    };

    http.post("https://discord.com/api/oauth2/token")
        .form(&request)
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await
}

pub struct DiscordUser {
    pub id: Id<UserMarker>,
    pub username: String,
}

#[derive(Deserialize)]
struct RawDiscordUser {
    id: String,
    username: String,
}

#[derive(Debug)]
pub enum FetchCurrentUserError {
    Request(reqwest::Error),
    InvalidUserId(String),
}

impl From<reqwest::Error> for FetchCurrentUserError {
    fn from(source: reqwest::Error) -> Self {
        Self::Request(source)
    }
}

pub async fn fetch_current_user(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<DiscordUser, FetchCurrentUserError> {
    let raw = http
        .get("https://discord.com/api/users/@me")
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json::<RawDiscordUser>()
        .await?;

    let id = raw
        .id
        .parse()
        .map_err(|_| FetchCurrentUserError::InvalidUserId(raw.id.clone()))?;

    Ok(DiscordUser {
        id,
        username: raw.username,
    })
}
