use std::fmt::Write as _;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

/// How long a dashboard login stays valid before the owner has to sign in
/// again. Much longer than the one-shot member-verification session
/// (`sessions::SessionStore`, 10 minutes) — this is meant to persist
/// across a normal browsing session, not just one redirect round-trip.
const SESSION_EXPIRY: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a dashboard login's CSRF state token stays valid — matches
/// the member-verification flow's own window.
const LOGIN_STATE_EXPIRY: Duration = Duration::from_secs(10 * 60);

struct Session {
    user_id: Id<UserMarker>,
    expires_at: Instant,
}

/// Signed-cookie-free session store for the owner dashboard: a random
/// token (set as an HttpOnly cookie) maps to a logged-in Discord user ID.
///
/// This is deliberately a different mechanism from
/// `verification::sessions::SessionStore`, even though both are
/// DashMap-backed random tokens — that one is a single-use CSRF token
/// tied to a specific guild's verification attempt; this one is a
/// longer-lived login that persists across many unrelated page loads and
/// isn't tied to any one guild.
#[derive(Default)]
pub struct DashboardSessionStore {
    sessions: DashMap<String, Session>,
    login_states: DashMap<String, Instant>,
}

impl DashboardSessionStore {
    /// Generates a CSRF state token for a dashboard login attempt, to be
    /// round-tripped through Discord's OAuth `state` parameter.
    pub fn create_login_state(&self) -> String {
        let token = random_token();
        self.login_states.insert(token.clone(), Instant::now());
        token
    }

    /// Consumes a login-attempt state token — one-time use, same as the
    /// member-verification flow's tokens.
    pub fn consume_login_state(&self, token: &str) -> bool {
        let Some((_, created_at)) = self.login_states.remove(token) else {
            return false;
        };

        created_at.elapsed() <= LOGIN_STATE_EXPIRY
    }

    /// Starts a logged-in session for `user_id`, returning the token to
    /// set as the session cookie's value.
    pub fn create_session(&self, user_id: Id<UserMarker>) -> String {
        let token = random_token();
        self.sessions.insert(
            token.clone(),
            Session {
                user_id,
                expires_at: Instant::now() + SESSION_EXPIRY,
            },
        );
        token
    }

    /// The logged-in user for a session cookie value, if it's still
    /// valid. Unlike `consume_login_state`, this does *not* remove the
    /// session — it needs to keep working across many page loads until
    /// it naturally expires.
    pub fn user_id(&self, token: &str) -> Option<Id<UserMarker>> {
        let session = self.sessions.get(token)?;

        if session.expires_at < Instant::now() {
            return None;
        }

        Some(session.user_id)
    }
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::fill(&mut bytes[..]);

    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut token, "{byte:02x}").expect("writing to a String should never fail");
    }

    token
}
