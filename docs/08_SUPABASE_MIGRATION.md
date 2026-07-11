# Blackwall — Supabase Postgres migration

Records the move from a local SQLite file to a shared Supabase Postgres
database, done in response to the user's decision to build a Vercel-hosted
marketing site with a live owner dashboard (`blackwallsite`, see that
repo's own README) rather than relying solely on the bot's own
axum-served `/dashboard`. A single shared Postgres database means the
website can read real data without the bot needing to expose a second
API for it.

## Why not just query the bot from the website?

The website is stateless serverless functions (Vercel); the bot is one
long-running process on a VPS with no public API beyond the axum web
server it already runs. Rather than build a second API surface for the
dashboard to call, both sides read/write the same Postgres database
directly — the bot as the sole writer (an event happens, it writes a
row), the website as a read-only consumer (a page loads, it queries).

## What changed

- **`Cargo.toml`**: `sqlx`'s `sqlite` feature dropped for `postgres` +
  `tls-rustls` (Supabase requires TLS; `tls-rustls` maps to
  `tls-rustls-ring`, matching the project's existing `ring`-only crypto
  provider setup from Stage 4 — no `aws_lc_rs` mixed in).
- **`config.rs`**: `DATABASE_PATH` (optional, SQLite file path) replaced
  by `DATABASE_URL` (required, a real Postgres connection string).
  Deliberately required, not optional-with-a-local-fallback: there's no
  sensible local-file fallback for a database another whole application
  also reads from.
- **`storage/database.rs`**: no longer creates tables. The schema now has
  exactly one authoritative source —
  `blackwallsite/supabase/schema.sql`, run once by hand in Supabase's SQL
  editor — instead of two `CREATE TABLE` copies (one SQLite-flavored in
  Rust, one Postgres-flavored in the website repo) that would drift
  apart the moment either changed. `connect()` still fails loudly and
  immediately if the `guilds` table is missing, with a message pointing
  at the schema file, rather than a confusing error the first time a
  real event needs it.
- **`storage/models.rs`**: every query rewritten for Postgres — `?`
  placeholders to `$1, $2, ...`, `INSERT OR REPLACE`/`INSERT OR IGNORE`
  to `ON CONFLICT ... DO UPDATE`/`DO NOTHING`. `GuildSettings` gained
  `#[derive(Clone)]` for the new cache (below). New
  `upsert_security_score()` writes the `security_scores` table the
  dashboard reads, avoiding sqlx's `json` feature by binding
  pre-serialized JSON text with a `::jsonb` cast in the query — same
  pattern already used for `backups.backup_json`.
- **`storage/cache.rs`** (new): `SettingsCache`, an in-memory
  `DashMap<GuildId, GuildSettings>` wrapping `models::get_guild_settings`.
  `guild_settings` is checked on *every* message, join, and audit-log
  entry — without this, migrating from a local SQLite file to a networked
  Postgres database would have turned a free local read into a network
  round-trip on the hottest path in the bot. Populated lazily on first
  read per guild; invalidated at the one place a `guild_settings` row is
  actually mutated after creation (`/setup`'s `support_server_join`
  toggle — see `discord::setup::toggle_support_join`).
- **`gateway.rs`**: all three `get_guild_settings` call sites (message
  create, member add, audit-log entry) now go through
  `state.settings_cache.get(...)` instead of calling `models::` directly.
- **`discord/security_score.rs` and `discord/setup.rs`**: both now call
  `models::upsert_security_score()` after computing
  `PermissionFindings`, matching `schema.sql`'s own comment that the
  table is "refreshed whenever `/security-score` runs or `/setup` changes
  something."
- **Dead code removed, not carried forward**: `guilds.lockdown_enabled`
  and `models::set_lockdown_enabled()` were write-only — grepped the
  whole codebase to confirm nothing ever read the column before dropping
  it from the new schema and deleting the two call sites in
  `actions::lockdown`. `guild_settings.verification_enabled` was likewise
  dropped: declared in the old SQLite schema but never read by any query
  in `models.rs`.

## Credential handling

Two *different* Supabase credentials exist for two different connection
methods to the same database, and mixing them up doesn't work:

- The website (`blackwallsite`) uses `@supabase/supabase-js`, which talks
  to Supabase's PostgREST HTTP API using the `service_role` key (a JWT)
  as a bearer credential.
- The bot uses `sqlx`, which needs the native Postgres wire protocol —
  a real `postgres://...` connection string with the database password
  set at project creation, from Supabase's Project Settings -> Database
  -> Connection string -> URI. The `service_role` key doesn't work here
  at all; it's not a database password.

Direct connection (not Supabase's pooler) is the right choice for the
bot specifically: Blackwall is one persistent process holding its own
small connection pool via `sqlx::PgPool`, not a swarm of short-lived
serverless invocations, so there's nothing for an external pooler to
help with — and Supabase's pooler defaults to PgBouncer's transaction
mode, which doesn't support the prepared statements `sqlx` issues by
default.

Per the user's explicit instruction, the `service_role` key and (once
available) `DATABASE_URL` are never written into source or committed —
only into gitignored local `.env`/`.env.local` files for this session's
own testing, and into Vercel's/the bot's own environment-variable
settings for deployment.

## Verification performed

- `cargo build`, `cargo clippy --all-targets`, `cargo fmt` — all clean.
- Connected directly to the live Supabase project with the
  `service_role` key (via a throwaway Node script using
  `@supabase/supabase-js`, not the Rust bot) and confirmed all 7 tables
  from `schema.sql` exist and are queryable.
- **Not yet done**: an actual live connection from the Rust bot itself.
  That needs `DATABASE_URL` — the direct Postgres connection string,
  distinct from the `service_role` key above — which hadn't been
  provided yet when this note was written. Once it is: connect, run
  `/setup` and `/security-score` against a real test server, and confirm
  the resulting rows are visible both via a direct query and through the
  dashboard website.

## What's still open

- No periodic timer refreshes `security_scores` — it's only written on
  `/setup` and `/security-score`. A server that never reruns either
  command keeps whatever score was last computed, which could go stale
  if permissions change through Discord's own UI in between. Acceptable
  for a first version; a scheduled re-check is a reasonable follow-up.
- `verified_users`, `lockdown_snapshots`, and `backups` all migrated
  schema-wise but haven't been exercised against the live Supabase
  project yet, same caveat as above — only a direct table-existence
  check has been done so far, not a real read/write through the bot.
