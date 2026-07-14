use std::future::Future;
use std::pin::Pin;

use futures_util::stream::{self, StreamExt};

/// A single boxed, type-erased action future. Boxing here is a one-time,
/// tiny heap allocation per action — utterly negligible next to the
/// actual network round-trip each of these represents. Erasing the type
/// also sidesteps a real Rust compiler quirk: keeping `dispatch` generic
/// over the concrete anonymous future type each call site's `async move`
/// block produces triggers a "Send is not general enough" higher-ranked
/// lifetime error once it's called from inside another `async fn` that's
/// itself spawned via `tokio::spawn` — a known rough edge where the
/// borrow-checker infers a single concrete lifetime for the captured
/// references instead of a for-all-lifetimes bound. Boxing gives it one
/// concrete type to reason about instead.
pub type BoxedAction<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Runs a batch of boolean-producing actions with bounded concurrency —
/// `true` for each one that succeeded — and returns `(succeeded, failed)`.
///
/// This is the replacement for a sequential `for item in items { item.await }`
/// loop wherever an action fans out into many independent Discord REST
/// calls (locking every channel in a guild, stripping every dangerous
/// role an actor holds). Detection in this codebase already runs in
/// nanoseconds to low microseconds (see `moderation::raid`'s own
/// benchmark); a sequential loop over N REST calls each paying full
/// round-trip latency was the actual bottleneck between "raid detected"
/// and "server actually locked down" — not anything in the detectors.
///
/// Running the futures concurrently instead doesn't bypass Discord's
/// rate limits: `twilight_http::Client`'s ratelimiter still paces every
/// request per-bucket underneath this. What changes is that N requests
/// are in flight as fast as that ratelimiter allows, instead of paying
/// N full round-trips back-to-back.
pub async fn dispatch(actions: Vec<BoxedAction<'_>>, concurrency: usize) -> (usize, usize) {
    let results: Vec<bool> = stream::iter(actions)
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let succeeded = results.iter().filter(|ok| **ok).count();
    let failed = results.len() - succeeded;

    (succeeded, failed)
}
