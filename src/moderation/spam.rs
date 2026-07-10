use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::util::Timestamp;

/// How far back message history is kept per user, per guild. Anything
/// older than this is dropped the next time that user posts — this bounds
/// memory use per user without needing a separate cleanup task.
const WINDOW: Duration = Duration::from_secs(10);

/// More than this many messages inside `WINDOW` counts as a burst.
const BURST_THRESHOLD: usize = 6;

/// This many *identical* messages in a row (within `WINDOW`) counts as
/// copy-paste spam.
const REPEAT_THRESHOLD: usize = 3;

/// This many user mentions in a single message counts as mention spam.
const MENTION_THRESHOLD: usize = 5;

/// How long a user is timed out for when a violation is actioned.
pub const TIMEOUT_DURATION: Duration = Duration::from_secs(10 * 60);

struct MessageEvent {
    at: Instant,
    content: String,
}

/// Tracks recent message activity per (guild, user) so patterns that span
/// multiple messages — bursts, copy-pasted repeats — can be detected.
///
/// Stored once in `AppState` and shared across every message, the same
/// spirit as the scam matcher: one tracker, not one rebuilt per message.
#[derive(Default)]
pub struct SpamTracker {
    activity: DashMap<(Id<GuildMarker>, Id<UserMarker>), VecDeque<MessageEvent>>,
}

/// Why a message tripped the anti-spam filter — used to build the log
/// embed and pick a human-readable reason.
pub enum SpamViolation {
    MentionSpam { count: usize },
    MessageBurst { count: usize },
    RepeatedMessages { count: usize },
}

impl SpamViolation {
    pub fn description(&self) -> String {
        match self {
            SpamViolation::MentionSpam { count } => format!("{count} mentions in one message"),
            SpamViolation::MessageBurst { count } => format!("{count} messages within 10 seconds"),
            SpamViolation::RepeatedMessages { count } => {
                format!("the same message posted {count} times in a row")
            }
        }
    }
}

/// The point in time a timeout applied right now should end.
pub fn timeout_until() -> Timestamp {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is set before 1970")
        .as_secs();

    let until_secs = now_secs + TIMEOUT_DURATION.as_secs();

    Timestamp::from_secs(until_secs as i64)
        .expect("computed timeout timestamp was outside Discord's valid range")
}

impl SpamTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a message and checks whether it — together with that
    /// user's recent history in this guild — trips a spam rule.
    ///
    /// Only one violation is ever returned per call, even if a message
    /// technically trips more than one rule; there's no need to escalate
    /// harder than "act on it" for an MVP.
    pub fn check(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        content: &str,
        mention_count: usize,
    ) -> Option<SpamViolation> {
        if mention_count >= MENTION_THRESHOLD {
            return Some(SpamViolation::MentionSpam {
                count: mention_count,
            });
        }

        let mut history = self.activity.entry((guild_id, user_id)).or_default();
        let now = Instant::now();

        while let Some(oldest) = history.front() {
            if now.duration_since(oldest.at) > WINDOW {
                history.pop_front();
            } else {
                break;
            }
        }

        history.push_back(MessageEvent {
            at: now,
            content: content.to_string(),
        });

        // How many of the most recent messages are identical to this one.
        // Empty content (e.g. an attachment-only message) is never treated
        // as a "repeat" — plenty of legitimate messages have no text.
        let repeat_count = if content.is_empty() {
            0
        } else {
            history
                .iter()
                .rev()
                .take_while(|event| event.content == content)
                .count()
        };

        if repeat_count >= REPEAT_THRESHOLD {
            return Some(SpamViolation::RepeatedMessages {
                count: repeat_count,
            });
        }

        if history.len() >= BURST_THRESHOLD {
            return Some(SpamViolation::MessageBurst {
                count: history.len(),
            });
        }

        None
    }
}
