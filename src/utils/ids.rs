use twilight_model::id::Id;
use twilight_model::util::Timestamp;

/// Discord's own epoch: 2015-01-01T00:00:00.000Z, in milliseconds since
/// the Unix epoch. Every Discord snowflake ID encodes its creation time
/// as milliseconds *since this point*, not since 1970 — that's the one
/// non-obvious fact this function exists to hide.
const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;

/// Decodes the creation time embedded in any Discord snowflake ID (user,
/// guild, channel, message — they're all built the same way): the top 42
/// bits, after discarding the low 22 bits used for worker/process/sequence
/// numbers, are milliseconds since [`DISCORD_EPOCH_MILLIS`].
///
/// Used by anti-raid to answer "how old is this account?" without an
/// extra Discord API call — the timestamp is already sitting inside the
/// ID Discord already sent us.
pub fn snowflake_created_at<T>(id: Id<T>) -> Timestamp {
    let unix_millis = (id.get() >> 22) + DISCORD_EPOCH_MILLIS;

    Timestamp::from_secs((unix_millis / 1000) as i64)
        .expect("a snowflake's decoded creation time should always be a valid timestamp")
}
