# Blackwall — Progress Log

A stage-by-stage account of what was actually built, in what order, and
why — including the two course corrections that happened along the way.
Read `00_VISION.md` for the target end-state and `01_ARCHITECTURE.md` for
the resulting technical shape; this document is the narrative connecting
"what was asked for" to "what exists now."

Environment note for reproducibility: this was built on Windows 11, with
Rust installed via `winget install --id Rustlang.Rustup -e`
(`rustc 1.96.1`), inside a Git Bash shell where `cargo`/`rustc` needed
`export PATH="$PATH:/c/Users/<user>/.cargo/bin"` prefixed to commands
because the shell session predated the PATH update. The project lives at
`C:\Blackwall`, git-initialized by `cargo init` but with no commits made
during any of this work (all changes are still in the working tree).

## Stage 1 — Scaffold + gateway connection

Created via `cargo init --name blackwall`, then dependencies added one at
a time with `cargo add <crate> [--features ...]` rather than hand-editing
`Cargo.toml`, so each addition's actual resolved version/feature set could
be inspected as it happened (important — see
`04_GOTCHAS_AND_LEARNINGS.md` #1).

Built:
- `main.rs` — loads config, builds the HTTP client, fetches the bot's own
  application ID (`http.current_user_application().await?.model().await?.id`
  — avoids needing a separate `CLIENT_ID` env var since Discord's
  application ID *is* the OAuth client ID, a fact that also matters for
  Stage 5), registers commands, builds `AppState`, hands off to
  `gateway::run`.
- `config.rs` — `Config::load()`, reading `DISCORD_TOKEN` (required) and
  `TEST_GUILD_ID` (optional).
- `state.rs` — `AppState { http, application_id }` (grows every stage
  since).
- `gateway.rs` — connects with `Intents::GUILDS | Intents::GUILD_MESSAGES
  | Intents::MESSAGE_CONTENT`, logs every non-bot message to the terminal,
  dispatches `Event::InteractionCreate` to `discord::interactions::handle`.
  `MESSAGE_CONTENT` is a **privileged intent** — must be toggled on in the
  Developer Portal (Bot → Privileged Gateway Intents) or the gateway
  connection is refused. This is documented prominently in the project
  README because it's the single most common "why won't my bot see
  message content" beginner trap.
- `discord/http.rs`, `discord/commands.rs`, `discord/interactions.rs` —
  one `/ping` command, registered and handled, to prove the full
  register → user runs it → bot replies pipeline end to end before
  building anything real on top of it.

Command registration detail worth preserving: `TEST_GUILD_ID`, if set,
routes registration through `interaction_client.set_guild_commands(guild_id,
&commands)` instead of `set_global_commands(&commands)` — guild-scoped
commands propagate in seconds, global ones can take up to an hour. This
became the standing local-dev pattern for the whole project.

## Stage 2 — Anti-scam phrase detection

`moderation/scam.rs`: a flat `&[Pattern]` list (category + phrase pairs,
~30 entries across Fake Nitro gift / Discord impersonation domains /
Steam scam / free Robux-gift-card scam / token grabber-IP logger / fake
giveaway wording), compiled once via `AhoCorasick::builder()
.ascii_case_insensitive(true).build(phrases)` in `build_matcher()`, called
exactly once in `main.rs` and stored in `AppState.scam_matcher`. `check()`
is a pure function: matcher + `&str` in, `Option<ScamMatch>` out.

`gateway.rs` gained: on every non-bot message, run `scam::check`; on a
match, delete the message and (if a log channel was configured) send a
`discord::embeds::scam_message_deleted` embed there.

**First log-channel design (later reworked):** a single `LOG_CHANNEL_ID`
env var, read into `Config`/`AppState`, used for every guild the bot was
in. This was a knowing, deliberate MVP shortcut at the time — the right
call for "prove message deletion + logging works at all" — but it doesn't
scale to more than one server, and that gap is exactly what triggered the
Stage 3/4 reordering below.

**Security incident (caught, not shipped):** partway through this stage,
the user pasted their real bot token directly into `.env.example` instead
of `.env`. `.env.example` is tracked by git (only `.env` is git-ignored),
so this would have committed a live secret on the next `git add`/`commit`.
It was caught immediately because `git status`/`git log` were checked
before any commit existed in the repo at all — nothing had to be scrubbed
from history. Fixed by creating `.env` with the real values and blanking
`.env.example` back to an empty template. See
`04_GOTCHAS_AND_LEARNINGS.md` #5 for the generalized lesson.

## Course correction #1 — "this isn't made for one server btw"

After Stage 2, the user pointed out the single global `LOG_CHANNEL_ID`
(and, relatedly, `TEST_GUILD_ID` gating which guild gets commands at all)
meant the bot fundamentally assumed one server, when the whole point of
Blackwall is to be installed on many independent servers, each wanting
their own log channel/roles/settings. Given the choice between "keep
building Stage 3 (anti-spam) on top of the broken assumption" and "fix
the foundation first," **Stage 4 (the database + `/setup`) was pulled
forward ahead of Stage 3.** This is preserved here explicitly because
it's a good example of the "MVP-first" principle in `00_VISION.md`
correctly overriding the *literal* numbered stage order when a genuine
architectural correctness issue is found — the stage numbers are a means
to disciplined incremental delivery, not a contract to follow blindly
past a known defect.

## Stage 4 — `/setup`, guild settings storage (built before Stage 3)

Added `sqlx` (`--no-default-features --features sqlite,runtime-tokio` —
see `04_GOTCHAS_AND_LEARNINGS.md` #9 for why the defaults were trimmed).
Built `storage/{mod,database,models}.rs` per `01_ARCHITECTURE.md`'s
database section (schema reproduced there). `AppState` gained `db:
SqlitePool`; `Config` gained `database_path` (default `"blackwall.db"`).

Built `discord/setup.rs`:
- Guild-only (`CommandBuilder::contexts([InteractionContextType::Guild])`).
- Gated on the invoker having `MANAGE_GUILD` or `ADMINISTRATOR` (read from
  `interaction.member.permissions`, which Discord computes and sends for
  free — no manual role-hierarchy math needed).
- Three optional options: `log_channel` (Channel, restricted to
  `ChannelType::GuildText`), `verified_role` (Role), `quarantine_role`
  (Role).
- Resolution logic for each of the three: use the option if given → else
  find an existing channel/role matching by name (`"log"` substring for
  the channel; exact case-insensitive `"Verified"`/`"Quarantine"` for the
  roles) → else create one (`create_role` with `Permissions::empty()`;
  `create_guild_channel(..., "blackwall-logs")` with
  `ChannelType::GuildText`).
- Permission-risk checks (inline, not yet extracted to
  `moderation/permissions.rs` — see `01_ARCHITECTURE.md`'s note on this
  planned retrofit): `@everyone` checked for `ADMINISTRATOR` /
  `CREATE_INVITE` / `MENTION_EVERYONE`; members with `ADMINISTRATOR` (via
  any role, or being the guild owner) counted via one `guild_members`
  fetch capped at 1000.
- Persists via `storage::models::upsert_guild_config` (upserts `guilds`;
  `INSERT OR IGNORE`s a default `guild_settings` row).
- Replies with an ephemeral `embeds::setup_summary` embed, which
  deliberately lists "Active now" (only anti-scam, at the time) separately
  from "Configured (activates in a later stage)" (anti-spam/raid/nuke/
  verification) — an explicit honesty choice so a server owner is never
  told a protection is live when it isn't yet. **Keep this pattern**: any
  future summary/status embed should distinguish "actually enforced right
  now" from "toggled on for when it's implemented."

`LOG_CHANNEL_ID` was removed entirely (superseded by the per-guild
`guilds.log_channel_id` looked up via `storage::models::get_log_channel_id`).

**Bug found via smoke test, fixed same stage:** the very first real
`cargo run` after this stage's changes panicked at startup with
`Could not automatically determine the process-level CryptoProvider`
from `rustls`. Neither `cargo build` nor `cargo clippy` had shown any
problem — this is a runtime-only failure, because `rustls` 0.23+ requires
an explicit `CryptoProvider::install_default()` call before the first TLS
connection, and nothing in the dependency tree was making that call
automatically. Fixed by adding `rustls` as a direct dependency
(`--no-default-features --features ring,std,tls12,logging` — explicitly
*excluding* the `aws_lc_rs` feature, which is a **default** feature of the
`rustls` crate itself and would otherwise have been pulled in alongside
`ring`, recreating the exact same ambiguity) and calling
`rustls::crypto::ring::default_provider().install_default().expect(...)`
as the very first line of `main()`. Full story, and the open (tested,
not-yet-fully-resolved) question of whether this needs revisiting once
`reqwest` is added in Stage 5, is in `04_GOTCHAS_AND_LEARNINGS.md` #3 —
**read it before adding anything else that touches TLS/networking.**

## Stage 3 — Anti-spam (built after Stage 4, on the now-per-server
foundation)

Built `moderation/spam.rs`:
- `SpamTracker` — `DashMap<(Id<GuildMarker>, Id<UserMarker>),
  VecDeque<MessageEvent>>`, `#[derive(Default)]`, built once in `main.rs`
  and stored in `AppState.spam_tracker` (same "build once, share via
  AppState" pattern as the scam matcher).
- Three rules, evaluated in `check()`: **mention spam** (≥5 mentions in
  one message — checked independent of history), **message burst** (≥6
  messages within a rolling 10-second window per user-per-guild),
  **repeated messages** (the same non-empty content 3+ times in a row
  within that same window). Only one violation is ever returned per call.
- `timeout_until()` computes "now + 10 minutes" as a
  `twilight_model::util::Timestamp` (`Timestamp::from_secs(unix_seconds)`)
  for Discord's native member-timeout field
  (`communication_disabled_until`) — no separate mute-role mechanism
  needed.
- Memory-bound reasoning documented in the code: each user's `VecDeque` is
  pruned to the 10-second window on every message they send, so per-user
  memory stays small; the outer `DashMap` does grow by one entry per
  unique `(guild, user)` ever seen and is never swept, which is accepted
  as proportional-to-real-usage rather than a true unbounded leak (same
  reasoning applied to the (still hypothetical, not yet built) join/nuke
  trackers described in the roadmap).

`gateway.rs` gained `handle_spam_violation` (delete + timeout + log embed,
mirroring `handle_scam_message`'s shape) and — this is the important
retrofit — **both** `anti_scam_enabled` and `anti_spam_enabled` are now
actually read from `storage::models::get_guild_settings` before running
either detector. Those two columns had existed in the `guild_settings`
table since Stage 4, but nothing read them until this stage — Stage 2's
scam filter ran unconditionally the whole time in between. This was
caught and fixed as part of implementing Stage 3's own toggle, rather than
being a separate bug hunt; it's called out here so future stages don't
repeat the pattern of adding a settings column without also wiring up the
read.

Explicitly deferred, and why (recorded so it isn't "silently forgotten,"
per the no-silent-caps norm): emoji/attachment/sticker/invite-link-volume
spam and Zalgo-text detection weren't built — the four rules implemented
cover the highest-signal, easiest-to-get-right cases for an MVP, and
adding the rest is mechanical repetition of the same `SpamTracker` shape,
not a design problem. A staff/whitelist bypass (a moderator can currently
be timed out by the bot like anyone else) was deliberately *not* built by
guessing at whether Discord populates `message.member.permissions` on
regular gateway messages (it's documented as an interaction-only field;
behavior on `MESSAGE_CREATE` wasn't verified against source before this
session ended) — the honest call was to rely on generous thresholds for
now and let the planned `/whitelist` command (already in the original
command list) be the real fix, rather than build a bypass on an unverified
assumption. **If you pick this up: verify `PartialMember.permissions`'
actual behavior on `MESSAGE_CREATE` against the installed crate source
before either building a bypass on it or ruling it out for good.**

Bot invite permissions needed an update at this point too: **Timeout
Members** (for the anti-spam action) plus **Manage Messages** (needed to
delete other users' messages, distinct from a bot's own messages) were
added to the documented required invite-permission list, alongside the
already-required Manage Roles / Manage Channels / Send Messages / Read
Message History from Stage 4.

## Current file inventory (end of Stage 3/4, start of Stage 5)

```
Cargo.toml
Cargo.lock
.env                (real secrets, git-ignored)
.env.example        (blank template, tracked)
.gitignore
README.md           (user-facing setup + architecture explanation)
docs/                (this document set)
blackwall.db         (SQLite file, git-ignored, created at runtime)
src/
  main.rs
  config.rs
  state.rs
  gateway.rs
  discord/
    mod.rs
    http.rs
    commands.rs
    interactions.rs
    setup.rs
    embeds.rs
  moderation/
    mod.rs
    scam.rs
    spam.rs
  storage/
    mod.rs
    database.rs
    models.rs
```

Every file above has been verified, in this session, to actually compile
(`cargo build`), pass lint (`cargo clippy --all-targets`, zero warnings),
be formatted (`cargo fmt --check`, clean), and — critically — actually run
against live Discord credentials (`RUST_LOG=info timeout 20 cargo run`,
confirmed gateway connection + command registration + "Blackwall is
online" in the logs) at each stage boundary. `/setup` and the anti-spam
timeout action were built and are believed correct against the verified
API surface, but their *end-to-end live behavior* (does `/setup` actually
create the right channel/roles when clicked in Discord; does a real spam
burst actually get timed out) has not been confirmed by the human at the
time of this log entry — flagged honestly rather than assumed.

## Stage 5 — in progress at the time this document set was written

Work had started (dependencies being added: `axum`, `reqwest`, `rand`)
when the user asked to pause and produce this documentation set instead.
**This is a known, currently-unresolved in-progress state**, recorded
precisely so it can be picked back up correctly:

- `axum` was added successfully (`cargo add axum`, defaults accepted:
  `form, http1, json, matched-path, original-uri, query, tokio,
  tower-log, tracing` features) — this resolved cleanly and is safe to
  build on as-is.
- `reqwest`'s dependency add took two tries. First attempt (`cargo add
  reqwest --no-default-features --features rustls-tls,json`) failed
  outright — `rustls-tls` is not a valid feature name on this reqwest
  version (`0.13.4`); the actual available TLS-related feature is named
  `rustls`. Second attempt (`cargo add reqwest --no-default-features
  --features rustls,json`) succeeded, but pulled in `aws-lc-rs`/
  `aws-lc-sys` as a transitive dependency (visible in the `cargo add`
  output: `Adding aws-lc-rs v1.17.1`, `Adding aws-lc-sys v0.42.0`, plus
  `cmake`/`dunce`/`fs_extra` needed to build it) — a **second** crypto
  backend alongside the project's existing `ring`-only `rustls`
  dependency, which is the same shape of conflict that broke the build
  once already in Stage 4 (`04_GOTCHAS_AND_LEARNINGS.md` #3).
  **However**: this was checked empirically (not just reasoned about)
  before pausing for documentation — both `cargo build` and a full smoke
  test (`RUST_LOG=info timeout 20 cargo run`, confirmed gateway connects,
  commands register, "Blackwall is online" logs) **succeed as-is**, with
  `axum`/`reqwest`/`rand` present in `Cargo.toml` but not yet referenced
  by any code. This is expected, not a fluke: the ambiguity-panic path
  only triggers when something tries to auto-select a provider *without*
  one already having been installed, and `main.rs`'s existing
  `rustls::crypto::ring::default_provider().install_default()` call
  already resolves the process-wide default before that path is ever
  reached — `aws-lc-rs` merely being compiled into the binary, unused,
  doesn't by itself cause a problem. **The genuinely open question is
  only what happens once `reqwest` is actually used to make a request**
  (does its TLS setup respect the process-wide default that's already
  installed, or does it force its own?) — this is untested, because no
  code calls `reqwest` yet. `03_ROADMAP.md` Stage 5 "Step 0" gives a
  five-minute throwaway-request test to answer this directly, and the
  two candidate fixes (in preference order: standardize the whole
  project on `aws-lc-rs` since that's what `reqwest` wants by default; or
  find a `reqwest` feature that selects `ring` specifically) to apply
  *only if* that test actually shows a conflict. **Do this test as the
  first action of resuming Stage 5**, before writing any other code.
- `rand` was added successfully (`cargo add rand`, defaults:
  `alloc, std, std_rng, sys_rng, thread_rng` — this is the crate intended
  for generating the OAuth `state` CSRF token; not yet used in any code).
- No `verification/` or `web/` source files exist yet. No new database
  table (`verified_users`) exists yet. No new env vars have been added to
  `config.rs` yet. Everything described for Stage 5 in `03_ROADMAP.md` is
  *plan*, not yet code.

## Stage 5 — completed in the resumed implementation pass

Stage 5 was later resumed and completed. The `reqwest`/`rustls` probe
returned `200 OK`, so the project stayed on the existing `ring` provider.
The implementation added `verification/`, `web/`, `/verify-panel`,
`verified_users`, early `security_events` recording, and the new optional
web/OAuth env vars. See `05_STAGE_5_COMPLETION.md` for the exact files,
behavior, verification evidence, and remaining human OAuth click-through
test.
