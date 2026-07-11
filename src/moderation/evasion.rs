use std::time::{Duration, Instant};

use dashmap::DashMap;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

/// How long after a ban or kick new joins get extra scrutiny. Long enough
/// to catch someone registering an alt and rejoining in the same sitting,
/// short enough that unrelated joins days later never get flagged.
const WINDOW: Duration = Duration::from_secs(30 * 60);

/// Tracks the most recent ban/kick per guild, so a new, individually
/// suspicious join shortly afterward can be flagged as a possible
/// evasion attempt.
///
/// Deliberately timing-only: Blackwall has no way to prove a new Discord
/// account belongs to the same person as a removed one — that would need
/// an IP address or device fingerprint, both of which raise far bigger
/// privacy questions than a join-timing correlation is worth (see
/// `docs/` for the reasoning this was scoped down from). This is a
/// signal for staff to look at, never grounds for an automatic block —
/// see `gateway::handle_possible_evasion`, which only ever logs.
#[derive(Default)]
pub struct EvasionTracker {
    last_removal: DashMap<Id<GuildMarker>, Instant>,
}

impl EvasionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_removal(&self, guild_id: Id<GuildMarker>) {
        self.last_removal.insert(guild_id, Instant::now());
    }

    /// Seconds since the guild's last recorded ban/kick, if it happened
    /// within `WINDOW` — `None` means either no recent removal or it's
    /// aged out of the window.
    pub fn seconds_since_removal(&self, guild_id: Id<GuildMarker>) -> Option<u64> {
        let last = self.last_removal.get(&guild_id)?;
        let elapsed = last.elapsed();

        (elapsed <= WINDOW).then_some(elapsed.as_secs())
    }
}
