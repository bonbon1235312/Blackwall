use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

/// Opens (creating the file if it doesn't exist yet) the SQLite database at
/// `path` and makes sure every table Blackwall needs is present.
///
/// This uses plain `CREATE TABLE IF NOT EXISTS` statements rather than a
/// migration framework. That's the right amount of complexity while the
/// schema is this small — if it grows a lot across many stages, moving to
/// `sqlx::migrate!` at that point is a reasonable follow-up.
pub async fn connect(path: &str) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options)
        .await
        .expect("failed to open the SQLite database");

    create_tables(&pool).await;

    pool
}

async fn create_tables(pool: &SqlitePool) {
    // IDs are stored as TEXT, not INTEGER. Discord IDs ("snowflakes") are
    // 64-bit numbers; storing them as text sidesteps any question of
    // whether they fit in SQLite's signed integer type, at the cost of a
    // `.to_string()` / `.parse()` at the Rust boundary (see storage/models.rs).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS guilds (
            guild_id            TEXT PRIMARY KEY,
            owner_id            TEXT NOT NULL,
            log_channel_id      TEXT,
            verified_role_id    TEXT,
            quarantine_role_id  TEXT,
            lockdown_enabled    INTEGER NOT NULL DEFAULT 0,
            created_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await
    .expect("failed to create the guilds table");

    // One row per guild, created with sensible defaults the first time
    // /setup runs for that guild (see models::upsert_guild_config). Kept
    // as its own table (rather than columns on `guilds`) because this is
    // exactly the shape described in the project's DATABASE TABLES spec,
    // and keeps "identity/config" separate from "feature toggles".
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS guild_settings (
            guild_id               TEXT PRIMARY KEY REFERENCES guilds(guild_id),
            anti_spam_enabled      INTEGER NOT NULL DEFAULT 1,
            anti_scam_enabled      INTEGER NOT NULL DEFAULT 1,
            anti_raid_enabled      INTEGER NOT NULL DEFAULT 1,
            anti_nuke_enabled      INTEGER NOT NULL DEFAULT 1,
            verification_enabled   INTEGER NOT NULL DEFAULT 1,
            support_join_enabled   INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await
    .expect("failed to create the guild_settings table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS verified_users (
            guild_id     TEXT NOT NULL,
            user_id      TEXT NOT NULL,
            verified_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            method       TEXT NOT NULL,
            PRIMARY KEY (guild_id, user_id)
        )",
    )
    .execute(pool)
    .await
    .expect("failed to create the verified_users table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS security_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            guild_id    TEXT NOT NULL,
            user_id     TEXT,
            event_type  TEXT NOT NULL,
            severity    TEXT NOT NULL,
            description TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await
    .expect("failed to create the security_events table");

    // One row per text channel, written the moment /lockdown (or an
    // automatic raid response) engages, and consumed (read + deleted) by
    // /unlockdown. `had_overwrite = 0` means the channel had no @everyone
    // permission overwrite before lockdown at all — restoring means
    // deleting the overwrite entirely, not writing back all-zero bits.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lockdown_snapshots (
            guild_id        TEXT NOT NULL,
            channel_id      TEXT NOT NULL,
            had_overwrite   INTEGER NOT NULL,
            everyone_allow  INTEGER,
            everyone_deny   INTEGER,
            PRIMARY KEY (guild_id, channel_id)
        )",
    )
    .execute(pool)
    .await
    .expect("failed to create the lockdown_snapshots table");

    // One row per /backup run. The whole snapshot is one JSON blob
    // (`backup_json`) rather than normalized role/channel tables —
    // matches the flat shape in the original spec, and is far simpler to
    // reason about than a relational model of "roles, channels,
    // overwrites, categories" with their own foreign keys.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS backups (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            guild_id    TEXT NOT NULL,
            backup_json TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await
    .expect("failed to create the backups table");
}
