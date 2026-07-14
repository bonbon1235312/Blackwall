use std::time::Duration;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

/// How long Blackwall waits for a `BotAdd` audit-log entry to attribute
/// who added a bot before defaulting to treating the add as unauthorized.
/// Audit-log entries are not instantaneous — this bounds how long an
/// unattributed bot can sit fully joined before Blackwall acts, instead of
/// waiting indefinitely on an event that can lag or (rarely) never arrive.
pub const AUDIT_LOG_GRACE: Duration = Duration::from_secs(3);

enum PendingState {
    MemberSeen,
    ActorKnown(Id<UserMarker>),
}

/// What the caller should do after `member_seen` — resolve the gate right
/// away (the audit-log entry already arrived, out of order), or wait for
/// it (start the grace-period fallback).
pub enum MemberSeenOutcome {
    AwaitAuditLog,
    ActorKnown(Id<UserMarker>),
}

/// Resolves the race between `Event::MemberAdd` (a bot joining — near
/// instant) and `Event::GuildAuditLogEntryCreate(BotAdd)` (who added it,
/// which can lag behind by an unbounded amount, and can even arrive
/// first). Whichever event lands first stashes a partial fact keyed by
/// `(guild_id, bot_id)`; whichever lands second completes it and drains
/// the entry. See `gateway::handle_bot_member_add` /
/// `gateway::handle_bot_add_audit_entry`.
#[derive(Default)]
pub struct BotAddGate {
    pending: DashMap<(Id<GuildMarker>, Id<UserMarker>), PendingState>,
}

impl BotAddGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call when `Event::MemberAdd` reports a bot joining.
    pub fn member_seen(&self, guild_id: Id<GuildMarker>, bot_id: Id<UserMarker>) -> MemberSeenOutcome {
        match self.pending.entry((guild_id, bot_id)) {
            Entry::Occupied(entry) => match *entry.get() {
                PendingState::ActorKnown(actor_id) => {
                    entry.remove();
                    MemberSeenOutcome::ActorKnown(actor_id)
                }
                // A duplicate MemberAdd for the same bot (shouldn't
                // normally happen) — nothing new to resolve.
                PendingState::MemberSeen => MemberSeenOutcome::AwaitAuditLog,
            },
            Entry::Vacant(entry) => {
                entry.insert(PendingState::MemberSeen);
                MemberSeenOutcome::AwaitAuditLog
            }
        }
    }

    /// Call when a `BotAdd` audit-log entry names `actor_id` as the adder
    /// of `bot_id`. Returns `true` if `MemberAdd` had already registered
    /// (the caller should evaluate the gate now); `false` if this is new
    /// information stashed for a `MemberAdd` that hasn't arrived yet — the
    /// caller should also schedule a `discard` after `AUDIT_LOG_GRACE` so
    /// a `MemberAdd` that never arrives (e.g. the bot's own join event was
    /// dropped) doesn't leave the entry orphaned forever.
    pub fn actor_known(
        &self,
        guild_id: Id<GuildMarker>,
        bot_id: Id<UserMarker>,
        actor_id: Id<UserMarker>,
    ) -> bool {
        match self.pending.entry((guild_id, bot_id)) {
            Entry::Occupied(entry) => {
                entry.remove();
                true
            }
            Entry::Vacant(entry) => {
                entry.insert(PendingState::ActorKnown(actor_id));
                false
            }
        }
    }

    /// Called after `AUDIT_LOG_GRACE` elapses from a `MemberSeen` insert.
    /// Returns `true` if the entry was still an unresolved `MemberSeen` —
    /// the caller should treat this as unattributed and act defensively.
    /// Returns `false` if it was already resolved (or never existed).
    pub fn take_if_still_pending(&self, guild_id: Id<GuildMarker>, bot_id: Id<UserMarker>) -> bool {
        matches!(
            self.pending.remove(&(guild_id, bot_id)),
            Some((_, PendingState::MemberSeen))
        )
    }

    /// Best-effort cleanup for a stashed `ActorKnown` fact whose matching
    /// `MemberAdd` never showed up within the grace period (e.g. the bot
    /// left again immediately). Not calling this would only leak one
    /// small map entry per such event, never grow unbounded under normal
    /// operation — but it costs nothing to clean up properly.
    pub fn discard(&self, guild_id: Id<GuildMarker>, bot_id: Id<UserMarker>) {
        self.pending.remove(&(guild_id, bot_id));
    }
}
