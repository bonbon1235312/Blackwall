use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::mpsc;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

use crate::storage::models::{self, SecurityEventRecord, VerificationRecord};

/// Work that's safe to defer past the moment a verifying user's browser
/// gets redirected to `/success` — none of it blocks Discord's own state
/// (the Verified role, already granted synchronously before either of
/// these is ever sent — see `verification::roles::grant_verified_role`
/// and `web::routes::attempt_support_join`), it's bookkeeping.
pub enum VerificationEvent {
    Completed {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        method: &'static str,
    },
    SecurityEvent {
        guild_id: Id<GuildMarker>,
        user_id: Option<Id<UserMarker>>,
        event_type: &'static str,
        severity: &'static str,
        description: String,
    },
}

/// How often pending events are flushed to Postgres as one batched
/// statement per event kind, instead of one round-trip per event — the
/// same 500ms cadence a non-blocking Postgres batcher would use anywhere
/// else in Blackwall, kept here since verification bursts are the one
/// place that's actually been observed to matter.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Runs forever, draining `rx` and periodically batch-writing whatever
/// accumulated since the last tick. Spawn exactly once, in `main.rs`;
/// `AppState` only ever holds the `Sender` half. Returns only when every
/// `Sender` has been dropped, which in practice means the process is
/// shutting down.
pub async fn run(db: PgPool, mut rx: mpsc::UnboundedReceiver<VerificationEvent>) {
    let mut completed: Vec<VerificationRecord> = Vec::new();
    let mut security_events: Vec<SecurityEventRecord> = Vec::new();

    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    // The first tick fires immediately — skip it so the very first batch
    // isn't flushed with (at most) one event in it.
    interval.tick().await;

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(VerificationEvent::Completed { guild_id, user_id, method }) => {
                        completed.push((guild_id, user_id, method));
                    }
                    Some(VerificationEvent::SecurityEvent { guild_id, user_id, event_type, severity, description }) => {
                        security_events.push((guild_id, user_id, event_type, severity, description));
                    }
                    None => return,
                }
            }
            _ = interval.tick() => {
                flush(&db, &mut completed, &mut security_events).await;
            }
        }
    }
}

async fn flush(
    db: &PgPool,
    completed: &mut Vec<VerificationRecord>,
    security_events: &mut Vec<SecurityEventRecord>,
) {
    if !completed.is_empty() {
        if let Err(source) = models::record_verifications_batch(db, std::mem::take(completed)).await {
            tracing::error!(?source, "failed to batch-write verification records");
        }
    }

    if !security_events.is_empty() {
        if let Err(source) =
            models::record_security_events_batch(db, std::mem::take(security_events)).await
        {
            tracing::error!(?source, "failed to batch-write security events");
        }
    }
}
