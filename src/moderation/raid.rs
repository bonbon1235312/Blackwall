use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use twilight_model::guild::Member;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;
use twilight_model::util::Timestamp;

use crate::utils::ids::snowflake_created_at;

/// How far back join history is kept per guild. Raids unfold over a
/// somewhat longer timescale than a single message-spam burst, hence the
/// longer window than `moderation::spam`'s 10 seconds. Not configurable —
/// `/config` exposes the count thresholds below, not the window itself.
const WINDOW: Duration = Duration::from_secs(60);

/// Fallback burst/suspicious thresholds for a guild with no `guild_settings`
/// row yet — must match `guild_settings.raid_burst_threshold` /
/// `raid_suspicious_threshold`'s own column defaults in schema.sql, and
/// what the `#[cfg(test)]` module below exercises.
pub const DEFAULT_BURST_THRESHOLD: usize = 10;
pub const DEFAULT_SUSPICIOUS_ACCOUNT_THRESHOLD: usize = 5;

/// An account created less than this long ago is treated as suspiciously
/// new for the purposes of anti-raid (separately from Stage 5/6's
/// verification flow, which is about identity, not account age).
const NEW_ACCOUNT_THRESHOLD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Hard memory bound for one guild's rolling join window. Detection trips
/// long before this limit, so retaining more records would only make a raid
/// consume extra memory without improving the decision.
const MAX_TRACKED_JOINS: usize = 1_000;

/// If Discord rejects the initial lockdown before any channel work starts,
/// allow one new incident attempt after this delay instead of retrying on
/// every join and amplifying an outage.
const INCIDENT_RETRY_DELAY: Duration = Duration::from_secs(10);

/// Fallback raid-response timeout duration for a guild with no
/// `guild_settings` row yet — must match
/// `guild_settings.raid_timeout_minutes`'s own column default.
pub const DEFAULT_RAID_TIMEOUT_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

/// The per-guild thresholds `/config` can adjust, read from `GuildSettings`
/// at the call site and passed in by value — small and `Copy`, so this adds
/// no allocation to the hot path `record` runs on.
#[derive(Clone, Copy)]
pub struct RaidThresholds {
    pub burst_threshold: usize,
    pub suspicious_threshold: usize,
    pub timeout: Duration,
}

impl Default for RaidThresholds {
    fn default() -> Self {
        Self {
            burst_threshold: DEFAULT_BURST_THRESHOLD,
            suspicious_threshold: DEFAULT_SUSPICIOUS_ACCOUNT_THRESHOLD,
            timeout: DEFAULT_RAID_TIMEOUT_DURATION,
        }
    }
}

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

#[derive(Default)]
struct GuildJoinWindow {
    events: VecDeque<JoinEvent>,
    suspicious_count: usize,
    incident_active: bool,
    retry_after: Option<Instant>,
}

/// Tracks recent joins per guild so patterns that only show up across
/// multiple joins — bursts, waves of suspicious accounts — can be
/// detected. Same "build once, share via `AppState`" shape as
/// `moderation::spam::SpamTracker`.
#[derive(Default)]
pub struct JoinTracker {
    activity: DashMap<Id<GuildMarker>, GuildJoinWindow>,
}

/// A newly detected raid and the bounded timeline that caused it.
pub struct RaidIncident {
    pub violation: RaidViolation,
    pub window: Vec<JoinRecord>,
}

/// Result of recording one join. The latest record is always returned for
/// evasion checks; the more expensive timeline clone only exists when a new
/// raid incident crosses a threshold.
pub struct RaidCheck {
    pub latest: JoinRecord,
    pub raid_active: bool,
    pub incident: Option<RaidIncident>,
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
/// end. Same shape as `moderation::spam::timeout_until`, just with a
/// guild-configurable duration instead of a fixed constant.
pub fn raid_timeout_until(timeout: Duration) -> Timestamp {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is set before 1970")
        .as_secs();

    let until_secs = now_secs + timeout.as_secs();

    Timestamp::from_secs(until_secs as i64)
        .expect("computed raid timeout timestamp was outside Discord's valid range")
}

impl JoinTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules a bounded retry after a raid response failed before the
    /// lockdown could start. Successful incidents stay deduplicated until
    /// the rolling window falls below both thresholds.
    pub fn schedule_incident_retry(&self, guild_id: Id<GuildMarker>) {
        if let Some(mut state) = self.activity.get_mut(&guild_id) {
            if state.incident_active {
                state.retry_after = Some(Instant::now() + INCIDENT_RETRY_DELAY);
            }
        }
    }

    /// Records a join and checks whether the guild's current join window
    /// looks like a raid. A guild emits one incident while either threshold
    /// remains active, preventing every subsequent join from launching the
    /// same expensive lockdown response again.
    pub fn record(
        &self,
        guild_id: Id<GuildMarker>,
        member: &Member,
        thresholds: RaidThresholds,
    ) -> RaidCheck {
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

        let mut state = self.activity.entry(guild_id).or_default();
        let now = Instant::now();

        while let Some(oldest) = state.events.front() {
            if now.duration_since(oldest.at) > WINDOW {
                let expired = state
                    .events
                    .pop_front()
                    .expect("front event disappeared while pruning join history");
                if expired.record.is_suspicious {
                    state.suspicious_count = state.suspicious_count.saturating_sub(1);
                }
            } else {
                break;
            }
        }

        if state.incident_active
            && state
                .retry_after
                .is_some_and(|retry_after| now >= retry_after)
        {
            state.incident_active = false;
            state.retry_after = None;
        }

        if state.events.len() < thresholds.burst_threshold
            && state.suspicious_count < thresholds.suspicious_threshold
        {
            state.incident_active = false;
            state.retry_after = None;
        }

        while state.events.len() >= MAX_TRACKED_JOINS {
            let discarded = state
                .events
                .pop_front()
                .expect("join history exceeded its cap without a front event");
            if discarded.record.is_suspicious {
                state.suspicious_count = state.suspicious_count.saturating_sub(1);
            }
        }

        if record.is_suspicious {
            state.suspicious_count += 1;
        }
        state.events.push_back(JoinEvent {
            at: now,
            record: record.clone(),
        });

        let violation = if state.events.len() >= thresholds.burst_threshold {
            Some(RaidViolation::JoinBurst {
                count: state.events.len(),
            })
        } else if state.suspicious_count >= thresholds.suspicious_threshold {
            Some(RaidViolation::SuspiciousAccounts {
                count: state.suspicious_count,
            })
        } else {
            None
        };
        let raid_active = violation.is_some();

        let incident = if state.incident_active {
            None
        } else {
            violation.map(|violation| {
                state.incident_active = true;
                state.retry_after = None;
                let window = state
                    .events
                    .iter()
                    .map(|event| event.record.clone())
                    .collect();

                RaidIncident { violation, window }
            })
        };

        RaidCheck {
            latest: record,
            raid_active,
            incident,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;

    use twilight_model::guild::{Member, MemberFlags};
    use twilight_model::user::User;
    use twilight_model::util::ImageHash;

    use super::*;

    const OLD_ACCOUNT_ID: u64 = 175_928_847_299_117_063;

    fn member(has_avatar: bool) -> Member {
        Member {
            avatar: None,
            communication_disabled_until: None,
            deaf: false,
            flags: MemberFlags::empty(),
            joined_at: None,
            mute: false,
            nick: None,
            pending: false,
            premium_since: None,
            roles: Vec::new(),
            user: User {
                accent_color: None,
                avatar: has_avatar.then(|| {
                    ImageHash::parse(b"0123456789abcdef0123456789abcdef")
                        .expect("benchmark avatar hash must be valid")
                }),
                avatar_decoration: None,
                avatar_decoration_data: None,
                banner: None,
                bot: false,
                discriminator: 0,
                email: None,
                flags: None,
                global_name: None,
                id: Id::new(OLD_ACCOUNT_ID),
                locale: None,
                mfa_enabled: None,
                name: "raid-benchmark".to_owned(),
                premium_type: None,
                public_flags: None,
                system: None,
                verified: None,
            },
        }
    }

    #[test]
    fn burst_emits_one_incident_while_threshold_remains_active() {
        let tracker = JoinTracker::new();
        let guild_id = Id::new(1);
        let member = member(true);
        let thresholds = RaidThresholds::default();

        for _ in 0..thresholds.burst_threshold - 1 {
            let check = tracker.record(guild_id, &member, thresholds);
            assert!(!check.raid_active);
            assert!(check.incident.is_none());
        }

        let incident = tracker
            .record(guild_id, &member, thresholds)
            .incident
            .expect("the tenth join must trigger a burst incident");
        assert!(matches!(
            incident.violation,
            RaidViolation::JoinBurst {
                count: DEFAULT_BURST_THRESHOLD
            }
        ));
        assert_eq!(incident.window.len(), DEFAULT_BURST_THRESHOLD);

        for _ in 0..100 {
            let check = tracker.record(guild_id, &member, thresholds);
            assert!(check.raid_active);
            assert!(check.incident.is_none());
        }
    }

    #[test]
    fn suspicious_accounts_trigger_at_the_fifth_join() {
        let tracker = JoinTracker::new();
        let guild_id = Id::new(2);
        let member = member(false);
        let thresholds = RaidThresholds::default();

        for _ in 0..thresholds.suspicious_threshold - 1 {
            let check = tracker.record(guild_id, &member, thresholds);
            assert!(!check.raid_active);
            assert!(check.incident.is_none());
        }

        let incident = tracker
            .record(guild_id, &member, thresholds)
            .incident
            .expect("the fifth suspicious join must trigger an incident");
        assert!(matches!(
            incident.violation,
            RaidViolation::SuspiciousAccounts {
                count: DEFAULT_SUSPICIOUS_ACCOUNT_THRESHOLD
            }
        ));
    }

    #[test]
    fn failed_incident_can_retry_without_per_join_amplification() {
        let tracker = JoinTracker::new();
        let guild_id = Id::new(4);
        let member = member(true);
        let thresholds = RaidThresholds::default();

        for _ in 0..thresholds.burst_threshold {
            black_box(tracker.record(guild_id, &member, thresholds));
        }

        tracker.schedule_incident_retry(guild_id);
        assert!(
            tracker
                .record(guild_id, &member, thresholds)
                .incident
                .is_none()
        );

        {
            let mut state = tracker
                .activity
                .get_mut(&guild_id)
                .expect("guild join state must exist");
            state.retry_after = Some(Instant::now() - Duration::from_millis(1));
        }

        let retry = tracker.record(guild_id, &member, thresholds);
        assert!(retry.raid_active);
        assert!(retry.incident.is_some());
    }

    #[test]
    fn join_history_is_hard_capped() {
        let tracker = JoinTracker::new();
        let guild_id = Id::new(3);
        let member = member(true);
        let thresholds = RaidThresholds::default();

        for _ in 0..MAX_TRACKED_JOINS + 500 {
            black_box(tracker.record(guild_id, &member, thresholds));
        }

        let state = tracker
            .activity
            .get(&guild_id)
            .expect("guild join state must exist");
        assert_eq!(state.events.len(), MAX_TRACKED_JOINS);
    }

    #[derive(Debug)]
    struct LatencyStats {
        p50_ns: u64,
        p95_ns: u64,
        p99_ns: u64,
        max_ns: u64,
    }

    fn latency_stats(mut samples: Vec<u64>) -> LatencyStats {
        samples.sort_unstable();
        let percentile = |percent: usize| samples[(samples.len() - 1) * percent / 100];

        LatencyStats {
            p50_ns: percentile(50),
            p95_ns: percentile(95),
            p99_ns: percentile(99),
            max_ns: *samples.last().expect("latency samples must not be empty"),
        }
    }

    fn elapsed_ns(started: Instant) -> u64 {
        started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX)
    }

    /// Release-only microbenchmark for the pure, in-process detector. The
    /// ignored marker keeps timing assertions out of normal debug test runs.
    #[test]
    #[ignore = "run explicitly in release mode to validate anti-raid latency"]
    fn antiraid_latency_stays_in_microseconds() {
        const SAMPLE_COUNT: usize = 2_000;
        const MAX_P95_NS: u64 = 100_000;

        let normal_member = member(true);
        let suspicious_member = member(false);
        let thresholds = RaidThresholds::default();

        let cold_tracker = JoinTracker::new();
        let mut cold_samples = Vec::with_capacity(SAMPLE_COUNT);
        for index in 0..SAMPLE_COUNT {
            let guild_id = Id::new(10_000 + index as u64);
            let started = Instant::now();
            black_box(cold_tracker.record(guild_id, &normal_member, thresholds));
            cold_samples.push(elapsed_ns(started));
        }

        let burst_tracker = JoinTracker::new();
        let mut burst_samples = Vec::with_capacity(SAMPLE_COUNT);
        for index in 0..SAMPLE_COUNT {
            let guild_id = Id::new(20_000 + index as u64);
            for _ in 0..thresholds.burst_threshold - 1 {
                black_box(burst_tracker.record(guild_id, &normal_member, thresholds));
            }
            let started = Instant::now();
            black_box(burst_tracker.record(guild_id, &normal_member, thresholds));
            burst_samples.push(elapsed_ns(started));
        }

        let suspicious_tracker = JoinTracker::new();
        let mut suspicious_samples = Vec::with_capacity(SAMPLE_COUNT);
        for index in 0..SAMPLE_COUNT {
            let guild_id = Id::new(30_000 + index as u64);
            for _ in 0..thresholds.suspicious_threshold - 1 {
                black_box(suspicious_tracker.record(guild_id, &suspicious_member, thresholds));
            }
            let started = Instant::now();
            black_box(suspicious_tracker.record(guild_id, &suspicious_member, thresholds));
            suspicious_samples.push(elapsed_ns(started));
        }

        let saturated_tracker = JoinTracker::new();
        let saturated_guild_id = Id::new(40_000);
        for _ in 0..MAX_TRACKED_JOINS {
            black_box(saturated_tracker.record(saturated_guild_id, &normal_member, thresholds));
        }
        let mut saturated_samples = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let started = Instant::now();
            black_box(saturated_tracker.record(saturated_guild_id, &normal_member, thresholds));
            saturated_samples.push(elapsed_ns(started));
        }

        let cold = latency_stats(cold_samples);
        let burst = latency_stats(burst_samples);
        let suspicious = latency_stats(suspicious_samples);
        let saturated = latency_stats(saturated_samples);

        eprintln!(
            "anti-raid latency (ns): cold={cold:?}, burst={burst:?}, \
             suspicious={suspicious:?}, saturated={saturated:?}"
        );

        for (scenario, stats) in [
            ("cold", &cold),
            ("burst", &burst),
            ("suspicious", &suspicious),
            ("saturated", &saturated),
        ] {
            assert!(
                stats.p95_ns <= MAX_P95_NS,
                "{scenario} anti-raid p95 was {} ns, above the {} ns budget; \
                 full stats: p50={} ns, p99={} ns, max={} ns",
                stats.p95_ns,
                MAX_P95_NS,
                stats.p50_ns,
                stats.p99_ns,
                stats.max_ns
            );
        }
    }
}
