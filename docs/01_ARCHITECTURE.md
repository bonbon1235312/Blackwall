# Blackwall — Architecture (as built)

This describes the system as it actually exists after Stages 1–4 (plus
Stage 3, built after Stage 4 — see `02_PROGRESS_LOG.md`). If you're
resuming this project, this document plus `02_PROGRESS_LOG.md` should let
you reconstruct the entire codebase's shape and reasoning without needing
to read every file. Cargo.toml/version specifics that matter for
reproducing an identical build are called out explicitly, because this
project targets a crates.io snapshot from mid-2026 whose exact APIs may
not match a general model's training data — see
`04_GOTCHAS_AND_LEARNINGS.md` point 1 for the verification methodology
used throughout, which you should keep using.

## System shape

One Rust binary (`blackwall`) runs two long-lived async tasks under a
single `tokio` runtime:

1. **The gateway task** (`gateway::run`) — a persistent websocket
   connection to Discord that receives events (messages, interactions,
   eventually member joins/audit-log entries) and reacts to them.
2. **The web task** (`web::run`, from Stage 5 onward) — an `axum` HTTP
   server serving the verification website and OAuth callback.

Both tasks share one `Arc<AppState>` (`src/state.rs`), constructed once in
`main.rs` and cloned (cheaply — it's just an `Arc` bump) into every spawned
task/handler. This is the single most important structural decision in
the codebase: **there is exactly one shared state struct**, not a grab-bag
of globals or a service-locator pattern. Everything a handler needs comes
through `&AppState`.

Discord communication is split into two halves, and this split is
explained in the project's own README because it trips up beginners:

- **Gateway** (`twilight-gateway`) — receives events, cannot itself send
  requests.
- **HTTP** (`twilight-http`, wrapped in `discord::http::build_client`) —
  sends requests (post messages, register commands, create roles, time
  out members, etc.), does not receive events.

You listen on the gateway and act via HTTP. Every moderation action in
this codebase follows the same shape: gateway event in → `AppState`
lookups (matcher check / DB query) → `twilight_http` call(s) out → maybe a
log embed sent via another `twilight_http` call.

## `AppState` (`src/state.rs`)

```rust
pub struct AppState {
    pub http: Arc<HttpClient>,
    pub application_id: Id<ApplicationMarker>,
    pub scam_matcher: AhoCorasick,
    pub db: SqlitePool,
    pub spam_tracker: SpamTracker,
}
```

Fields are added here, and only here, when a new stage needs a new piece
of shared state. Nothing is duplicated into per-module globals. The
pattern for every "detector" (scam matcher, spam tracker, and — per the
roadmap — the future join tracker, nuke-actor tracker, verification
session store) is the same: **build it once at startup, store it in
`AppState`, never rebuild it per-event.** This was an explicit constraint
in the original brief for the Aho-Corasick matcher and has been applied
consistently to everything shaped like it since, because the reasoning
generalizes: rebuilding a matcher or losing accumulated per-user state on
every message would both waste work and break the detection logic itself
(bursts/repeats need memory across messages).

## Module tree (as built)

```
src/
  main.rs           Startup sequence (see below). ~50 lines, no logic of
                    its own beyond "load config, build clients, wire
                    AppState, hand off to gateway::run".
  config.rs         Config struct + Config::load(). Reads env vars via
                    dotenvy. Owns the `non_empty_env` helper (see
                    Gotcha #4).
  state.rs          AppState struct definition only. No behavior.
  gateway.rs        Connects to the gateway, the event-dispatch match
                    statement, and the per-detector "handle_X_violation"
                    action functions (delete message, apply Discord
                    action, send log embed). This file is where
                    detection results turn into consequences.
  discord/
    mod.rs          Re-exports submodules. No logic.
    http.rs         One function: build the twilight_http::Client.
    commands.rs     build_commands() (the full slash command list) +
                    register() (registers globally or to TEST_GUILD_ID).
                    Delegates each command's *definition* to that
                    command's own module (e.g. setup::command()) rather
                    than inlining every CommandBuilder call here — this
                    file stays a thin aggregator as the command count
                    grows.
    interactions.rs Routes an incoming InteractionCreate to a handler by
                    command name or setup-panel component ID. Trivial
                    commands (e.g. /ping) reply inline; anything with real
                    logic gets its own module.
    setup.rs        /setup: permission-gated, opens an ephemeral panel with
                    channel/role selectors and action buttons. Creates
                    defaults only after an admin clicks the panel button,
                    persists to storage::models, and runs a quick check.
    embeds.rs       Every embed builder in the bot lives here (not
                    scattered across the modules that trigger them) --
                    scam_message_deleted, spam_timeout, setup_panel
                    today. Keeps Discord-presentation concerns in one
                    place separate from detection/action logic.
  moderation/
    mod.rs          Re-exports submodules.
    scam.rs         Static categorized phrase list, build_matcher()
                    (called once, in main.rs), check() (pure function:
                    matcher + text in, Option<ScamMatch> out — no I/O).
    spam.rs         SpamTracker (DashMap-backed), SpamViolation enum,
                    check() (records + evaluates in one call), plus
                    timeout_until() (the only place that touches wall-clock
                    time, deliberately isolated so the "what counts as
                    spam" logic stays pure/testable in principle even
                    though no tests exist yet).
  storage/
    mod.rs          Re-exports submodules.
    database.rs     connect(path) -> SqlitePool: opens/creates the SQLite
                    file, runs CREATE TABLE IF NOT EXISTS for every table
                    (no migration framework — see Gotcha #9 / design
                    note below).
    models.rs       GuildConfig, GuildSettings structs, plus every query
                    function (upsert_guild_config, get_guild_config,
                    setup-panel setters, get_log_channel_id,
                    get_guild_settings). Each query function is a single
                    focused async fn, not a generic repository/DAO
                    abstraction — deliberately, per the "no premature
                    abstraction" principle.
```

Deliberate divergences from the original target tree
(`00_VISION.md` §14), and why:

- `discord/setup.rs` and `discord/interactions.rs` exist even though the
  original sketch only listed `discord/commands.rs` and `discord/embeds.rs`.
  Reasoning: once `/setup` needed real branching logic (permission checks,
  role/channel resolution, multiple failure-reply paths), inlining it into
  `commands.rs` would have made that file do two unrelated jobs
  (*defining* every command vs. *handling* one specific command's complex
  logic). Splitting per-command handler modules out, with `commands.rs`
  staying a thin registry and `interactions.rs` staying a thin router, is
  the same "small, focused, single-responsibility file" instinct the
  original architecture sketch was already going for — just applied one
  level deeper than the sketch spelled out. **Keep doing this**: as
  `/config`, `/lockdown`, `/security-score`, etc. get built, give each a
  own module under `discord/` (or promote to a `discord/commands/`
  subdirectory once there are enough of them that `discord/` itself feels
  crowded — not yet).
- `moderation/permissions.rs` does not exist yet even though the target
  tree lists it. The permission-risk-checking logic (checking `@everyone`
  for dangerous perms, counting Administrator members) currently lives
  inline inside `discord/setup.rs`. This is flagged as a **retrofit** in
  `03_ROADMAP.md` Stage 8: extract it before `/security-score` needs the
  same logic, so it isn't duplicated.
- `actions/` (delete.rs, timeout.rs, ban.rs, quarantine.rs, lockdown.rs)
  does not exist yet. So far, every "action" (delete a message, time out a
  member) is a two-or-three-line direct `twilight_http` call inline inside
  `gateway.rs`'s `handle_*_violation` functions. This is intentional —
  extracting a whole `actions/` module for a single-line HTTP call each
  would be the premature abstraction the principles explicitly warn
  against. **Revisit this once `/lockdown`, `/lockdown`'s snapshot/restore
  logic, and nuke-response actions arrive** (Stage 7/8) — at that point
  "quarantine a member" and "lock down a channel" are genuinely
  multi-step, reused-from-multiple-places operations, which is exactly
  when pulling them into `actions/` stops being premature and starts being
  correct.
- `verification/` and `web/` do not exist yet — Stage 5 territory, not yet
  started (see `03_ROADMAP.md`).
- `utils/` (time.rs, ids.rs, errors.rs) does not exist yet. Nothing has
  needed it: no custom error types yet (every fallible call handled inline
  with `match`/`if let Err` + `tracing::error!`, or `.expect()` at startup
  for unrecoverable config problems — see "Error handling philosophy"
  below), and the one wall-clock-time helper that exists
  (`spam::timeout_until`) lives next to its only caller. **Create
  `utils/ids.rs` when Stage 7 needs to decode a Discord snowflake's
  embedded creation timestamp** (for "is this a suspiciously new
  account?" raid detection) — that's the first genuine need for a
  shared, non-domain-specific utility function.

## Error handling philosophy (the two postures, and where each applies)

Two distinct error-handling styles are used throughout, deliberately, not
inconsistently:

1. **Fail loud at startup.** Anything read once at process start that the
   bot cannot meaningfully run without (`DISCORD_TOKEN` missing, the
   Discord application ID lookup failing, command registration failing,
   the scam pattern list failing to compile) uses `.expect("clear message")`
   and lets the process crash immediately with a readable panic message.
   Rationale stated in `config.rs`'s own doc comment: "without a valid
   token there is nothing useful the bot can do, so failing fast at
   startup is the right move." A confusing bot that silently limps along
   half-configured is worse than one that refuses to start.
2. **Fail soft at runtime, for anything triggered by a single Discord
   event.** If deleting one scam message fails, or sending one log embed
   fails, that must never crash the bot or drop other users' messages —
   log via `tracing::error!(?source, "...")` and continue. This is why
   every `handle_*` function in `gateway.rs` uses `if let Err(source) = ...
   { tracing::error!(...) }` rather than `?`/`.expect()` — a transient
   Discord API hiccup on one action must stay local to that one action.

When adding new code, ask "is this a one-time startup precondition, or a
per-event action?" and match the existing posture for whichever it is.

## Database (as built)

SQLite, opened/created via `storage::database::connect`. **No migration
framework** — every table is created with `CREATE TABLE IF NOT EXISTS` at
startup. This is a deliberate, documented-as-temporary simplification (see
`database.rs`'s own comment): appropriate while the schema is this small
and every table's shape is still being figured out stage-by-stage;
revisit (`sqlx::migrate!`) if/when the schema needs to *change* an
existing table's shape rather than just add new tables, since
`ALTER TABLE` by hand in a `CREATE TABLE IF NOT EXISTS`-only world is
fragile.

**Tables that exist right now:**

```sql
CREATE TABLE IF NOT EXISTS guilds (
    guild_id            TEXT PRIMARY KEY,
    owner_id            TEXT NOT NULL,
    log_channel_id      TEXT,
    verified_role_id    TEXT,
    quarantine_role_id  TEXT,
    lockdown_enabled    INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS guild_settings (
    guild_id               TEXT PRIMARY KEY REFERENCES guilds(guild_id),
    anti_spam_enabled      INTEGER NOT NULL DEFAULT 1,
    anti_scam_enabled      INTEGER NOT NULL DEFAULT 1,
    anti_raid_enabled      INTEGER NOT NULL DEFAULT 1,
    anti_nuke_enabled      INTEGER NOT NULL DEFAULT 1,
    verification_enabled   INTEGER NOT NULL DEFAULT 1,
    support_join_enabled   INTEGER NOT NULL DEFAULT 0
);
```

**Tables from the target schema (`00_VISION.md` §13) not yet created:**
`verified_users`, `security_events`, `backups`. Each is scoped to the
roadmap stage that first needs it — see `03_ROADMAP.md`. Do not create
them early "for completeness"; create each when the feature that writes
to it is actually being built, per the no-premature-abstraction principle.

**Why IDs are stored as `TEXT`, not `INTEGER`:** Discord snowflakes are
64-bit values. SQLite's `INTEGER` is a signed 64-bit type, so snowflakes
fit *today*, but storing as text sidesteps the question entirely (no risk
of a future snowflake exceeding `i64::MAX`, no risk of a sign-related
off-by-one surprise) at the cost of a `.to_string()` / `.parse()` at the
Rust/SQL boundary. `Id<T>` (from `twilight-model`) implements both
`Display` and `FromStr`, so this conversion is one line each way — see
`storage/models.rs`.

**Why no ORM / query builder / compile-time-checked `sqlx::query!`
macros:** `sqlx` was added with `--no-default-features --features
sqlite,runtime-tokio` specifically to exclude the `macros`/`derive`
features. The compile-time-checked macros need either a live database
connection at `cargo build` time or a `.sqlx` offline query cache — both
are extra ceremony that doesn't pay for itself yet at this project's
size, and would be one more thing to explain to a first-time Rust
developer. Every query is a plain runtime `sqlx::query(...).bind(...)`
call, decoded manually via `row.try_get::<T, _>("column_name")`. Revisit
only if query-related bugs start actually happening in practice that
compile-time checking would have caught — don't add it preemptively.

**Guild-settings-row existence semantics:** A guild has **no**
`guild_settings` row until `/setup` runs for it (`upsert_guild_config`
does `INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?)` as part
of its work, which is the only place a row gets created). Every read
(`get_guild_settings`) treats "no row" as "everything enabled" —
i.e., a server that hasn't run `/setup` yet still gets full protection by
default, matching the smart-defaults principle, rather than silently
protecting nothing until an admin discovers `/setup` exists.

## Command / interaction handling shape

Every slash command follows the same three-part shape:

1. **Definition** — a `pub fn command() -> Command` in that command's own
   module (or inline in `commands.rs` for trivial ones like `/ping`),
   built with `twilight_util::builder::command::CommandBuilder` and its
   per-option-type sub-builders (`ChannelBuilder`, `RoleBuilder`, etc.).
   `commands.rs::build_commands()` collects every command's definition
   into one `Vec<Command>`.
2. **Registration** — `commands.rs::register()`, called once at startup,
   registers either to `TEST_GUILD_ID` (near-instant propagation, for
   local development) or globally (production; can take up to an hour to
   reach every server). This is controlled by whether `TEST_GUILD_ID` is
   set in the environment — see `config.rs`.
3. **Handling** — `interactions.rs::handle()` matches on
   `command.name.as_str()` and either replies inline (trivial commands) or
   delegates to that command's own module's `handle()` function, which
   does its own work and sends its own response.

**Permission gating pattern**, established in `setup.rs` and intended to
be reused for every future admin-facing command: check
`interaction.member.as_ref().and_then(|m| m.permissions)` for the required
bit (`Permissions::MANAGE_GUILD` or `Permissions::ADMINISTRATOR` for
`/setup`). Discord computes and sends the invoking member's effective
permissions *for the channel the interaction happened in* as part of the
interaction payload — there is no need to manually walk role hierarchy or
channel overwrites to gate a command.

**Ephemeral-by-default for admin/config commands.** `/setup` opens its
panel with `MessageFlags::EPHEMERAL`, using
`InteractionResponseDataBuilder::flags(...)`. Admin/config output shouldn't
spam the channel for
every member to see; `/verify-panel`'s output (a persistent public panel)
is the deliberate exception, since its whole purpose is to be visible.

## Moderation detector shape

Both detectors built so far (`moderation::scam`, `moderation::spam`)
follow the same shape, and every future detector
(`moderation::raid`, `moderation::nuke`) should too:

1. A **pure `check()` function** (or method) that takes the minimal
   relevant input (message text; message text + mention count + who/where)
   and returns `Option<SomeViolationType>` — no I/O, no Discord calls, no
   logging inside the detector itself. This keeps the "is this bad?"
   question cleanly separated from "what do we do about it?"
2. **A `handle_*_violation` function living in `gateway.rs`** that takes
   the violation and performs the consequence: delete the message (nearly
   always), apply the Discord-side action (delete-only for scam; delete +
   timeout for spam), look up that guild's log channel from the database,
   build the embed (via `discord::embeds::*`), send it, and log everything
   via `tracing::warn!`/`tracing::error!` as appropriate. Every step here
   uses the "fail soft" error posture above.
3. **A toggle check before the detector even runs**, reading
   `guild_settings` (`settings.anti_scam_enabled` / `anti_spam_enabled`)
   so server owners can turn a whole category off. This wasn't wired up
   for `anti_scam_enabled` until Stage 3's implementation retroactively
   fixed it — see `02_PROGRESS_LOG.md` for why that gap existed and how it
   was caught. **When adding `moderation::raid` / `moderation::nuke`,
   gate them on `anti_raid_enabled` / `anti_nuke_enabled` from the start**
   — don't repeat the retrofit.

## Cargo dependency reference (exact, as of end of Stage 3/4 — before
Stage 5 work began)

```toml
[dependencies]
aho-corasick = "1.1.4"
dashmap = "6.2.1"
dotenvy = "0.15.7"
rustls = { version = "0.23.41", default-features = false, features = ["ring", "std", "tls12", "logging"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.150"
sqlx = { version = "0.9.0", default-features = false, features = ["sqlite", "runtime-tokio"] }
tokio = { version = "1.52.3", features = ["full"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
twilight-gateway = "0.17.1"
twilight-http = "0.17.1"
twilight-model = "0.17.1"
twilight-util = { version = "0.17.0", features = ["builder"] }
```

Toolchain: `rustc 1.96.1`, installed via `winget install --id
Rustlang.Rustup -e` (rustup, not a standalone rustc install) — on Windows,
`cargo`/`rustc` land in `%USERPROFILE%\.cargo\bin`, which needs adding to
`PATH` (or invoked with the full path / an `export PATH=...` prefix in
shells that started before the PATH change propagated).

The explicit direct dependency on `rustls` (with only the `ring` crypto
backend feature, `default-features = false`) exists **only** to call
`rustls::crypto::ring::default_provider().install_default()` once at the
top of `main()`, before any TLS connection (gateway websocket, any HTTPS
call) is opened. See `04_GOTCHAS_AND_LEARNINGS.md` #3 for the full story
— this is not optional, and it becomes *more* fragile, not less, once
`reqwest` is added in Stage 5. Read that gotcha before touching TLS-related
dependencies again.

## Configuration (env vars, as of end of Stage 4)

```
DISCORD_TOKEN     required. Bot token from the Developer Portal.
TEST_GUILD_ID     optional. Registers commands to one guild instantly
                  instead of globally (which can take up to an hour).
DATABASE_PATH     optional, defaults to "blackwall.db" in the working
                  directory.
```

All three are read via `config::non_empty_env` (see Gotcha #4) except
`DISCORD_TOKEN`, which is required and uses `.expect()` directly. `.env`
is loaded via `dotenvy::dotenv()` and is git-ignored; `.env.example` is
the tracked template and must **never** contain a real secret value (see
Gotcha #5).

## Testing/verification methodology used throughout (keep using this)

1. **`cargo build`** after every meaningful change — catches type errors,
   missing imports, wrong method signatures.
2. **`cargo clippy --all-targets`** — catches lint-level issues (this
   session used its suggestions directly, e.g. collapsing nested `if let`
   into a let-chain).
3. **`cargo fmt`** — applied after clippy is clean, then re-verified with
   a build (formatting-only diffs shouldn't change behavior, but confirm
   it anyway).
4. **A real smoke-test run**: `RUST_LOG=info timeout 20 cargo run` (or the
   PowerShell/platform equivalent), reading the actual log output for
   "connecting to the Discord gateway...", "Blackwall is online", and
   "registered slash commands...". This is **not optional** — it is the
   only one of these four steps that caught the rustls crypto-provider
   panic (Gotcha #3), which both `build` and `clippy` were blind to
   because it's a runtime-only failure mode. Any change that touches
   startup sequencing, dependency additions with their own TLS/crypto/
   network stack, or the gateway connection itself should get this
   smoke test before being considered done.
5. **Direct crate-source verification before writing code against an
   unfamiliar API** — see Gotcha #1. This project targets crate versions
   published after most models' training cutoffs; do not guess API shapes
   from memory when the exact installed source is one `grep` away.
