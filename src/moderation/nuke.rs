use std::collections::VecDeque;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use twilight_model::guild::audit_log::AuditLogEventType;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};

/// How far back dangerous-action history is kept per (guild, actor).
const WINDOW: Duration = Duration::from_secs(30);

/// This many dangerous actions by the same actor within `WINDOW` counts as
/// a nuke attempt.
const THRESHOLD: usize = 3;

/// Which audit-log event types are dangerous enough to count toward the
/// nuke threshold. Deliberately a fixed list, not "anything in the audit
/// log" — routine admin work (creating a channel, updating an emoji)
/// shouldn't ever trip this.
fn is_dangerous(action_type: AuditLogEventType) -> bool {
    matches!(
        action_type,
        AuditLogEventType::ChannelDelete
            | AuditLogEventType::RoleDelete
            | AuditLogEventType::MemberBanAdd
            | AuditLogEventType::MemberKick
            | AuditLogEventType::WebhookCreate
            | AuditLogEventType::RoleUpdate
            | AuditLogEventType::MemberRoleUpdate
            | AuditLogEventType::GuildUpdate
            | AuditLogEventType::BotAdd
    )
}

struct ActionEvent {
    at: Instant,
}

/// Tracks recent dangerous audit-log actions per (guild, actor) so a
/// burst of them — not just one, which could be legitimate admin
/// work — trips the anti-nuke response. Same "build once, share via
/// `AppState`" shape as `moderation::spam::SpamTracker` and
/// `moderation::raid::JoinTracker`.
#[derive(Default)]
pub struct NukeTracker {
    activity: DashMap<(Id<GuildMarker>, Id<UserMarker>), VecDeque<ActionEvent>>,
}

pub struct NukeViolation {
    pub count: usize,
}

impl NukeViolation {
    pub fn description(&self) -> String {
        format!("{} dangerous admin actions within 30 seconds", self.count)
    }
}

impl NukeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one audit-log entry by `actor_id`, if its action type is
    /// dangerous, and checks whether they've tripped the threshold.
    /// Non-dangerous action types are ignored entirely — nothing is
    /// recorded for them, so routine admin activity never counts toward
    /// the window.
    pub fn record(
        &self,
        guild_id: Id<GuildMarker>,
        actor_id: Id<UserMarker>,
        action_type: AuditLogEventType,
    ) -> Option<NukeViolation> {
        if !is_dangerous(action_type) {
            return None;
        }

        let mut history = self.activity.entry((guild_id, actor_id)).or_default();
        let now = Instant::now();

        while let Some(oldest) = history.front() {
            if now.duration_since(oldest.at) > WINDOW {
                history.pop_front();
            } else {
                break;
            }
        }

        history.push_back(ActionEvent { at: now });

        if history.len() >= THRESHOLD {
            Some(NukeViolation {
                count: history.len(),
            })
        } else {
            None
        }
    }
}
