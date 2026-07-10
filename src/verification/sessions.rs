use std::fmt::Write as _;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use twilight_model::id::{Id, marker::GuildMarker};

const STATE_TOKEN_BYTES: usize = 32;
const SESSION_EXPIRY: Duration = Duration::from_secs(10 * 60);

pub struct PendingVerification {
    pub guild_id: Id<GuildMarker>,
    pub created_at: Instant,
}

#[derive(Default)]
pub struct SessionStore {
    sessions: DashMap<String, PendingVerification>,
}

impl SessionStore {
    pub fn create(&self, guild_id: Id<GuildMarker>) -> String {
        let mut bytes = [0_u8; STATE_TOKEN_BYTES];
        rand::fill(&mut bytes[..]);

        let mut token = String::with_capacity(STATE_TOKEN_BYTES * 2);
        for byte in bytes {
            write!(&mut token, "{byte:02x}").expect("writing to a String should never fail");
        }

        self.sessions.insert(
            token.clone(),
            PendingVerification {
                guild_id,
                created_at: Instant::now(),
            },
        );

        token
    }

    pub fn take(&self, token: &str) -> Option<PendingVerification> {
        let (_, pending) = self.sessions.remove(token)?;

        if pending.created_at.elapsed() > SESSION_EXPIRY {
            return None;
        }

        Some(pending)
    }
}
