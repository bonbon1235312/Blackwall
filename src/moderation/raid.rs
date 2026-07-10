use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use twilight_model::guild::Member;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::util::Timestamp;

use crate::utils::ids::snowflake_created_at;

/// How far back join history is kept per guild. Raids unfold over a
/// somewhat longer timescale than a single message-spam burst, hence the
/// longer window than `moderation::spam`'s 10 seconds.
const WINDOW: Duration = Duration::from_secs(60);

/// This many joins within `WINDOW` counts as a raid-scale burst on its own,
/// regardless of how "normal" any individual joiner looks.
const BURST_THRESHOLD: usize = 10;

/// An account created less than this long ago is treated as suspiciously
/// new for the purposes of anti-raid (separately from Stage 5/6's
/// verification flow, which is about identity, not account age).
const NEW_ACCOUNT_THRESHOLD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// If at least this many of the joins currently in the window are
/// individually suspicious (new account and/or no avatar), that's treated
/// as a raid even if the raw join count hasn't hit `BURST_THRESHOLD` yet —
/// that combination is a stronger signal than volume alone.
const SUSPICIOUS_ACCOUNT_THRESHOLD: usize = 5;

/// How long a suspicious joiner is timed out for when a raid is detected.
/// Longer than a spam timeout (`moderation::spam::TIMEOUT_DURATION`)
/// deliberately: a raid is a much stronger signal of bad intent than a
/// single spam burst, and staff need real time to review before it lapses.
pub const RAID_TIMEOUT_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

/// One join, as recorded for the anti-raid window.
#[derive(Clone)]
pub struct JoinRecord {
    pub user_id: Id<UserMarker>,
    pub username: String,
    pub account_created_at: Timestamp,
    pub has_avatar: bool,
    /// New account, no avatar, or both — this specific join looked
    /// suspicious on its own, independent of how many others joined
    /// alongside it.
    pub is_suspicious: bool,
}

struct JoinEvent {
    at: Instant,
    record: JoinRecord,
}

/// Tracks recent joins per guild so patterns that only show up across
/// multiple joins — bursts, waves of suspicious accounts — can be
/// detected. Same "build once, share via `AppState`" shape as
/// `moderation::spam::SpamTracker`.
#[derive(Default)]
pub struct JoinTracker {
    activity: DashMap<Id<GuildMarker>, VecDeque<JoinEvent>>,
}

/// Why a join window tripped the anti-raid filter.
pub enum RaidViolation {
    JoinBurst { count: usize },
    SuspiciousAccounts { count: usize },
}

impl RaidViolation {
    pub fn description(&self) -> String {
        match self {
            RaidViolation::JoinBurst { count } => {
                format!("{count} joins within 60 seconds")
            }
            RaidViolation::SuspiciousAccounts { count } => {
                format!("{count} new-account or no-avatar joins within the last 60 seconds")
            }
        }
    }
}

/// The point in time a raid-response timeout applied right now should
/// end. Same shape as `moderation::spam::timeout_until`, just with the
/// longer `RAID_TIMEOUT_DURATION`.
pub fn raid_timeout_until() -> Timestamp {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is set before 1970")
        .as_secs();

    let until_secs = now_secs + RAID_TIMEOUT_DURATION.as_secs();

    Timestamp::from_secs(until_secs as i64)
        .expect("computed raid timeout timestamp was outside Discord's valid range")
}

impl JoinTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a join and checks whether the guild's current join window
    /// looks like a raid. Always returns the window's contents (for
    /// building a raid timeline) alongside any violation, since the
    /// window is small and bounded by `WINDOW` regardless of outcome.
    pub fn record(
        &self,
        guild_id: Id<GuildMarker>,
        member: &Member,
    ) -> (Vec<JoinRecord>, Option<RaidViolation>) {
        let account_created_at = snowflake_created_at(member.user.id);
        let now_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is set before 1970")
            .as_secs() as i64;
        let account_age_secs = now_unix_secs - (account_created_at.as_micros() / 1_000_000);
        let is_new_account = account_age_secs < NEW_ACCOUNT_THRESHOLD.as_secs() as i64;
        let has_avatar = member.user.avatar.is_some();

        let record = JoinRecord {
            user_id: member.user.id,
            username: member.user.name.clone(),
            account_created_at,
            has_avatar,
            is_suspicious: is_new_account || !has_avatar,
        };

        let mut history = self.activity.entry(guild_id).or_default();
        let now = Instant::now();

        while let Some(oldest) = history.front() {
            if now.duration_since(oldest.at) > WINDOW {
                history.pop_front();
            } else {
                break;
            }
        }

        history.push_back(JoinEvent {
            at: now,
            record: record.clone(),
        });

        let suspicious_count = history
            .iter()
            .filter(|event| event.record.is_suspicious)
            .count();

        let violation = if history.len() >= BURST_THRESHOLD {
            Some(RaidViolation::JoinBurst {
                count: history.len(),
            })
        } else if suspicious_count >= SUSPICIOUS_ACCOUNT_THRESHOLD {
            Some(RaidViolation::SuspiciousAccounts {
                count: suspicious_count,
            })
        } else {
            None
        };

        let window: Vec<JoinRecord> = history.iter().map(|event| event.record.clone()).collect();

        (window, violation)
    }
}
