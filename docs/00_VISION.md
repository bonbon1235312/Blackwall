# Blackwall — Project Vision

This document is the source brief for Blackwall. Everything else in `docs/`
is downstream of this file. If you are an AI or a developer picking this
project up without the rest of the conversation history that produced it,
read this first — it is the "why" behind every architectural choice
recorded in `01_ARCHITECTURE.md` and every task in `03_ROADMAP.md`.

## Elevator pitch

A fast, lightweight Discord security bot, written in Rust, with a
dead-simple verification website. It protects Discord servers from raids,
spam, scam links, suspicious accounts, nukes, dangerous permission
changes, and malicious bots. The main selling point is **simple setup,
smart defaults, and a clean owner dashboard** — not the biggest feature
list, not the flashiest UI.

## Non-negotiable principles

These apply to every stage of the build, not just the parts already done:

1. **Keep the Rust code understandable for a beginner.** The person
   driving this project is a first-time Rust developer. Every file should
   be explicable in plain language. Comments should explain *why*, never
   *what* — identifiers and structure should make the *what* obvious.
2. **Do not over-engineer early.** No abstractions for hypothetical future
   requirements. Three similar lines beat a premature trait/generic.
3. **Build an MVP first, then expand.** Follow the staged build order
   (below / in the roadmap) — don't skip ahead to advanced features while
   basics are still rough, and don't gold-plate a stage before moving on.
4. **Prefer clear architecture over extreme optimization.** Correctness
   and readability first. Rust's performance ceiling is high enough that
   "clean and slightly slower" almost always beats "clever and opaque."
5. **Avoid shady OAuth behavior.** Never hide scopes. Never silently
   perform an action a user didn't clearly consent to.
6. **Verification must clearly disclose what's being authorized.** If
   `guilds.join` is used, the user must be told — in visible text, not
   fine print — that continuing may add them to the bot's official
   support/community server.
7. **Minimal data retention.** Never store OAuth access tokens longer than
   the single request that needs them, unless refresh behavior is
   explicitly required (it currently isn't — refresh tokens are not
   stored anywhere in this design). Store only what's listed in the
   database schema below — nothing extra "just in case."
8. **Secrets only in environment variables**, never hardcoded, never
   logged, never committed. See `04_GOTCHAS_AND_LEARNINGS.md` for the
   incident where this almost went wrong and how it was caught.
9. **No dark patterns, anywhere.** No hidden costs, no pre-ticked boxes,
   no "pretend this is required when it isn't," no fake stats, no fake
   testimonials, no made-up "millions protected" marketing copy.

## Tech stack

**Bot (this repository's primary content):**
- Rust
- `tokio` — async runtime
- `twilight-gateway`, `twilight-http`, `twilight-model` — Discord API
  (chosen over `serenity`/`poise` deliberately: twilight is lower-level,
  which is more explicit and teachable for a beginner than a
  batteries-included framework that hides the request/response and
  gateway-event shapes)
- `twilight-util` (`builder` feature) — ergonomic builders for commands,
  embeds, interaction responses, message components (buttons)
- `twilight-interactions` — mentioned in the original brief as "where
  useful"; not yet needed, since command definitions so far are simple
  enough for `twilight-util`'s builders. Revisit if command option
  parsing gets complex enough to want derive-macro ergonomics.
- `dashmap` — concurrent per-user/per-guild in-memory state (spam
  tracking today; join tracking, nuke-actor tracking, etc. later)
- `aho-corasick` — compiled-once multi-pattern string matching for scam
  phrase detection (compiled at startup, never per-message — this
  constraint is explicit in the original brief and enforced throughout)
- `regex` — only where actually needed (not currently used anywhere;
  Aho-Corasick covers the literal-phrase-matching needs so far)
- `serde` / `serde_json` — data modeling and (de)serialization
- `dotenvy` — `.env` file loading for local development
- `tracing` / `tracing-subscriber` — structured logging
- `sqlx` — persistence, once needed (needed starting Stage 4)
- **SQLite** for now (PostgreSQL is explicitly a "later, only if actually
  needed" option in the original brief — do not switch preemptively)

**Website:**
- The original brief allowed Next.js/SvelteKit/Astro/plain HTML+Rust
  backend. **Decision made during this build: plain HTML/CSS rendered by
  a Rust backend (`axum`), living in the same binary as the bot**, rather
  than a separate JS project. Rationale: the target architecture
  (`14_RUST_ARCHITECTURE` in the original brief) already sketches a
  `web/` module inside `src/`, and keeping everything in one Rust
  codebase avoids introducing a second toolchain/language for a
  first-time Rust developer to context-switch into. See
  `01_ARCHITECTURE.md` for the concrete crate choices (`axum`, `reqwest`)
  and `03_ROADMAP.md` for the OAuth flow this enables.
- Minimal, fast, dark themed, professional. No template-engine
  dependency — pages are built as plain Rust functions returning HTML
  strings with an inline `<style>` block. This is deliberately low-tech;
  revisit only if the page count/complexity genuinely outgrows it.

## Core bot features (full list, target end-state)

### 1. Basic setup
Slash commands: `/setup`, `/config`, `/lockdown`, `/unlockdown`,
`/security-score`, `/backup`, `/restore`, `/verify-panel`, `/logs`,
`/whitelist`, `/blacklist`, `/test-security`.

`/setup` should: detect likely staff roles, detect admin roles, detect
dangerous permissions, ask for or auto-detect a log channel, ask for or
create a quarantine role, ask for or create a verified role, enable smart
default protections, and show a clean summary, e.g.:

> Security setup complete.
> Enabled: Anti-spam, Anti-scam, Anti-raid, Anti-nuke, Permission
> monitoring, Verification system.
> Warnings: 5 users have Administrator; @everyone can create invites; no
> backup exists yet.

### 2. Anti-spam
Detect: repeated messages, message bursts, mention spam, emoji spam,
sticker spam, attachment spam, link spam, invite spam, Zalgo/unicode
abuse, long message flooding, copy-paste raids. Use a leaky/token bucket
per user, state in `DashMap`. Actions: delete message, warn, timeout,
kick, ban, alert staff.

### 3. Scam and phishing detection
Aho-Corasick, compiled once at startup. Detect: fake Nitro links, token
grabber phrases, Steam scam links, free Robux/gift scams, Discord
impersonation links, suspicious shortened URLs, known scam domains, fake
giveaway wording.

### 4. Anti-raid
Detect: join bursts, many new accounts joining, no-avatar accounts
joining, suspicious usernames, bot join floods, repeated joins from
similar accounts. Actions: enable lockdown, increase verification level
if possible, pause invites if possible, require verification, timeout new
users, alert staff, log raid timeline.

### 5. Verification system
Discord flow: `/verify-panel` posts an embed with a Verify button →
website OAuth verification URL → website authorizes the Discord app →
bot gives the verified role on success.

Website OAuth flow: `/verify` page explains what the bot is, what data is
requested, what server is being verified for, and whether the user will
be invited to the official support/community server → "Continue with
Discord" → scopes `identify` (and `guilds.join` only if the support-server
join feature is enabled for that server) → exchange code for token →
verify state parameter → fetch identity → add verified role via the
**bot** token → optionally add to the support server (see below) →
redirect to success page.

Security requirements: OAuth `state` parameter, CSRF protection,
short-lived verification sessions, never expose tokens to the frontend,
never log tokens, store only: Discord user ID, server ID, verification
timestamp, verification result. Provide a privacy policy.

### 6. Support server join (handle carefully)

**Allowed version:** the verify page clearly states (not fine print):
"By continuing, you authorise this app to verify your Discord account.
This may also add you to our official support/community server so you
can receive support, updates, and security alerts." The OAuth consent
includes `guilds.join`. After authorization, the backend *may* call
Discord's add-guild-member endpoint for the official support server using
the user's access token. Already-a-member is a no-op success. If joining
fails, primary verification must still succeed unless support-server
membership is intentionally required (it should not be, by default).

**Do NOT:** secretly join users to a server; hide the join in small text;
store tokens for mass re-adding later; use the join as a dark pattern;
pretend it's required if it isn't.

### 7. Anti-nuke
Monitor (via gateway audit-log events where possible): mass channel
deletion, mass role deletion, mass bans, mass kicks, webhook creation,
admin permission grants, dangerous role edits, dangerous channel
permission edits, bot additions, server setting changes. Actions: remove
dangerous roles from the attacker, quarantine the attacker, ban/kick if
configured, lock the server, restore deleted channels/roles from a
backup if one exists, alert the owner.

### 8. Permission security
Detect: new Administrator grants, Manage Roles/Guild/Webhooks grants,
Mention Everyone grants, dangerous `@everyone` overwrites, staff
role-hierarchy risks, bot role positioned too low to protect the server.
`/security-score`: overall score out of 100, critical risks, medium
risks, suggested fixes.

### 9. Backup and restore
Back up: roles, role permissions, channel names/types/categories, channel
permission overwrites, basic server settings where possible. Restore:
recreate deleted channels/roles, restore permission overwrites, produce a
restore report.

### 10. Logging
Clean embeds for: message deleted by filter, user timed out,
banned/kicked, raid detected, lockdown enabled, dangerous permission
change, bot added, verification success/fail, OAuth support-server join
success/fail, backup created/restored.

### 11. Lockdown
`/lockdown`: stop unverified users from speaking, lock risky channels,
increase slowmode, disable invite creation if possible, alert staff.
`/unlockdown` safely reverts exactly what `/lockdown` changed.

### 12. Dashboard / website
Landing page: hero ("Fast Discord security, simple setup"), add-bot
button, support-server button, feature cards, screenshot/mockup section,
footer links. Verify page: as above. Success page: "You are verified" +
buttons to return to Discord / open the support server. Dashboard
(later): server list, security score, recent incidents, protection
toggles, verification settings, logs, backup status. Keep the first
website version extremely simple.

### 13. Database tables (target end-state)

```
guilds:
  guild_id, owner_id, log_channel_id, verified_role_id,
  quarantine_role_id, lockdown_enabled, created_at, updated_at

verified_users:
  guild_id, user_id, verified_at, method

security_events:
  id, guild_id, user_id (nullable), event_type, severity, description,
  created_at

guild_settings:
  guild_id, anti_spam_enabled, anti_scam_enabled, anti_raid_enabled,
  anti_nuke_enabled, verification_enabled, support_join_enabled

backups:
  id, guild_id, backup_json, created_at
```

(Exact SQL as actually implemented so far is in `01_ARCHITECTURE.md`;
tables not yet created are called out explicitly in `03_ROADMAP.md`.)

### 14. Rust architecture (target file tree)

```
src/
  main.rs
  config.rs
  state.rs
  gateway.rs
  discord/
    mod.rs
    http.rs
    commands.rs
    embeds.rs
  moderation/
    mod.rs
    spam.rs
    scam.rs
    raid.rs
    nuke.rs
    permissions.rs
  verification/
    mod.rs
    oauth.rs
    roles.rs
    sessions.rs
  web/
    mod.rs
    routes.rs
    templates.rs
  storage/
    mod.rs
    database.rs
    models.rs
  actions/
    mod.rs
    delete.rs
    timeout.rs
    ban.rs
    quarantine.rs
    lockdown.rs
  utils/
    time.rs
    ids.rs
    errors.rs
```

This is a target, not gospel — see `01_ARCHITECTURE.md` for where the
as-built tree has already reasonably diverged (e.g. `discord/setup.rs`
and `discord/interactions.rs` weren't in the original sketch but were the
right call once `/setup` got non-trivial) and use the same judgment going
forward: deviate when it's clearly better, not by default.

### 15. MVP build order

1. Rust project + gateway connection + terminal message logging +
   slash command registration.
2. Anti-scam phrase detection, delete bad messages, log embeds.
3. Anti-spam counters, timeout users, configurable thresholds.
4. `/setup`, guild settings storage, verified role configuration.
5. Verification website, OAuth `identify`, verified role on success.
6. Optional support-server join with clear disclosure; privacy policy +
   terms.
7. Anti-raid join monitoring, lockdown mode.
8. Anti-nuke monitoring, permission risk detection.
9. Backup/restore.
10. Dashboard.

**Note on actual build order:** Stages 3 and 4 were swapped in practice.
See `02_PROGRESS_LOG.md` for why — short version: the original single
global `LOG_CHANNEL_ID` design didn't scale past one server, and fixing
that (Stage 4, the database + `/setup`) was correctly done before piling
more moderation features (Stage 3) on top of a broken foundation. This is
the kind of judgment call this document should encourage, not discourage:
the *order* is a means to the MVP-first end, not the end itself.

### 16. Design style

Premium, clean, fast, security-focused. Not cheesy, not overloaded. Name
chosen: **Blackwall** (from a shortlist that also included Ironlock,
NullGuard, Sentinel, Watchtower, Lockbyte, HexGuard, RustShield,
SentryCore, Vaultix). Website style: dark background, subtle grid, soft
glowing border cards, one accent color, simple animations. No fake
stats, no fake testimonials, no made-up "millions protected."

### 17. First MVP success criteria

- Bot joins a test server.
- `/setup` works.
- Scam phrases are deleted.
- Spam gets timed out.
- Logs are sent to a channel.
- Verification button opens the website.
- OAuth verifies the user.
- Verified role is applied.
- Optional support-server join is clearly disclosed and works.
- Privacy policy exists.

### 18. Final instruction (the governing constraint on *how* to build)

Build step-by-step. Do not dump a massive unfinished codebase. Start with
the smallest working bot and website, then expand. Explain each file
simply because the developer is new to Rust. Prioritize correctness,
safety, and clean structure over extreme performance.
