# Blackwall — Stages 7–10 completion note

Covers Stages 7 (anti-raid + lockdown), 8 (anti-nuke + `/security-score`),
9 (`/backup`/`/restore`), and 10 (owner dashboard) as one note, since all
four were built back-to-back in one session in response to the user's
explicit "attempt to get all the features done and start testing"
instruction, rather than one at a time with a completion note in between.
All four are feature-complete, `cargo build`/`clippy --all-targets`/`fmt`
clean, and smoke-tested (see "Verification performed" below) — but **not
yet tested live against a real Discord server** the way Stages 1–6 were
in earlier sessions. That live test is the immediate next step, not
further building.

## Stage 7 — anti-raid + lockdown

- New gateway handling: `Event::MemberAdd`, gated by a new
  `guild_settings.anti_raid_enabled` column (defaults on). Needs the
  `GUILD_MEMBERS` privileged intent, already enabled since Stage 3's
  member-permission checks.
- `moderation::raid::JoinTracker` — a `DashMap<GuildId, Vec<JoinRecord>>`
  built once in `AppState`, same shape as `SpamTracker`. Each `JoinRecord`
  keeps `account_created_at` (decoded from the snowflake via the new
  `utils::ids::snowflake_created_at`) and `has_avatar`, not just a
  timestamp — both fields are read by `embeds::suspicion_note` so a raid
  alert says *why* an account looked suspicious instead of a bare label.
- Two independent trigger conditions, either one fires a response: 10+
  joins in 60 seconds (pure volume), or 5+ individually-suspicious joins
  (no avatar and/or account under 7 days old) in that window.
- Response: every text channel locked via the new `actions::lockdown`
  module (see below), the *individually suspicious* joiners from the
  triggering window timed out for 24 hours (not everyone caught in the
  burst — a fast but harmless wave of real users shouldn't be punished
  for volume alone), and a log embed with the specific join timeline.
- New shared module: `actions::lockdown::engage`/`revert`. `engage`
  fetches every text channel and, per channel, **snapshots the exact
  prior `@everyone` overwrite** (or notes there wasn't one) into the new
  `lockdown_snapshots` table before setting a `SEND_MESSAGES: deny`
  overwrite. `revert` reads the snapshot back and restores it exactly —
  or deletes the overwrite entirely if there wasn't one before — rather
  than naively clearing whatever's there now. This one module is called
  from three places: `/lockdown` (manual), the raid response, and (Stage
  8) the nuke response.
- New commands: `/lockdown`, `/unlockdown` — both **Manage Server**-gated,
  both logged.
- Owner-immunity: same pattern as Stage 3's spam fix — `get_owner_id` is
  checked before the raid-response timeout, since Discord refuses to
  timeout a guild owner regardless of bot permissions. (The owner
  triggering the *volume* half of raid detection by joining their own
  server is not actually possible — they're already a member — so this
  matters only in a multi-account testing scenario, not production, but
  the check costs nothing and keeps the code honest.)

## Stage 8 — anti-nuke + `/security-score`

- New gateway handling: `Event::GuildAuditLogEntryCreate`, gated by a new
  `guild_settings.anti_nuke_enabled` column (defaults on). This event's
  gating intent, `GUILD_MODERATION`, was verified against the installed
  `twilight-gateway` source before use (per this project's standing
  crate-verification discipline) and confirmed **not privileged** — no
  Developer Portal toggle needed, only the in-server `VIEW_AUDIT_LOG`
  bot permission.
- `moderation::nuke::NukeTracker` and `is_dangerous()` — matches audit log
  actions considered high-signal for a nuke: channel/role deletes, bans,
  kicks, webhook creation, role/permission edits, member role edits,
  guild settings edits, and a new bot being added. Tracked per
  `(guild_id, actor_id)`, not per-guild — a nuke is one account doing a
  lot of damage fast, so the actor is the thing that matters, not overall
  server activity.
- Trigger: 3+ dangerous actions by the *same actor* within 30 seconds.
- Response: strip every role from the actor that grants
  Administrator/Manage-Guild/Manage-Roles/Manage-Channels/
  Manage-Webhooks (via the new `moderation::permissions::dangerous_role_ids`
  helper, shared with `/security-score`), quarantine them (if a
  Quarantine role is configured) or fall back to a timeout, lock down
  every channel (reusing `actions::lockdown::engage` from Stage 7), DM the
  server owner, and log an embed. Owner-immunity applies here too — the
  actor being the owner is checked before any of this fires, since an
  owner's own dangerous-looking actions (e.g. cleaning up channels) are
  not a nuke and Discord wouldn't let the bot act on them anyway.
- New module: `moderation::permissions`, extracted from what used to be
  an inline function inside `discord/setup.rs`. Holds `PermissionFindings
  { critical, medium }`, the `check()` function (the `@everyone`/admin-
  count checks `/setup` already had), `dangerous_role_ids()` (new, used by
  anti-nuke), and — added at the end of this session, once the dashboard
  needed the same score `/security-score` computes — a `score()` method
  on `PermissionFindings` (100 minus 30 per critical finding, 10 per
  medium finding) so the scoring formula lives in exactly one place
  instead of being duplicated between the slash command and the
  dashboard.
- New command: `/security-score` — reruns `permissions::check()` plus one
  new check `/setup` doesn't do: is the bot's own highest role position
  above every dangerous role's position? If not, anti-nuke's role-strip
  and quarantine literally cannot work (Discord rejects role edits against
  a target whose role outranks the actor's), so this is flagged as
  critical. **Manage Server**-gated, ephemeral reply.

## Stage 9 — `/backup` / `/restore`

- New `backups` table: one row per backup, `guild_id` + a JSON blob +
  timestamp. `/backup` overwrites nothing — every run inserts a new row;
  `/restore` always reads the single most recent one for that guild.
- `RoleBackup`/`ChannelBackup`/`GuildBackup` (all `serde::Serialize` +
  `Deserialize`) capture: for roles, name/color/permissions/hoisted/
  mentionable/position, skipping `@everyone` and Discord-managed roles
  (bot/integration roles that can't be recreated meaningfully anyway);
  for channels, name/position/parent (category) restricted to
  `GuildText` and `GuildCategory` kinds — voice channels and other
  channel types are out of scope for this stage.
- `/restore` is explicitly **not** a full rollback: it recreates anything
  currently missing **by name** comparison against the backup, and the
  reply says outright that new objects get new Discord IDs, so anything
  elsewhere that referenced the old IDs (other channels' permission
  overwrites, external configuration) won't automatically follow. This
  was a deliberate scope decision, not an oversight — a byte-for-byte
  rollback (matching by ID, restoring exact overwrites/permissions on
  existing objects too) is a meaningfully bigger feature than what this
  stage's time budget allowed, and an honest partial tool beats an
  implied-but-untrue "full restore."
- Both commands **Manage Server**-gated.

## Stage 10 — owner dashboard

- New `axum` routes on the same conditionally-spawned web server as
  verification (`web::run`, still gated on `DISCORD_CLIENT_SECRET` being
  set): `GET /dashboard/login`, `GET /dashboard/callback`, `GET
  /dashboard`, `GET /dashboard/{guild_id}` (axum 0.8's `{param}` path
  syntax, verified against installed `axum` source before use, same as
  Stage 5).
- New module `verification::dashboard` — `DashboardSessionStore`, a
  second `DashMap`-backed store alongside `verification::sessions`, kept
  **deliberately separate** rather than reused: dashboard logins are
  24-hour sessions that persist across many unrelated page loads, member
  verification's `SessionStore` is a single-use 10-minute CSRF token tied
  to one guild's verify attempt. Conflating them risked one flow's
  lifetime assumptions leaking into the other.
- New OAuth entry point `oauth::dashboard_authorize_url` — also kept
  separate from `oauth::authorize_url` (member verification) even though
  both currently request only the `identify` scope, for the same reason:
  different purpose, different redirect URI, and if the dashboard ever
  needs a broader scope later, the two flows shouldn't have to be pulled
  apart retroactively.
- **Access control needed no new table and no `guilds` OAuth scope.**
  The original plan considered fetching `/users/@me/guilds` (requiring
  the `guilds` scope and temporarily holding the OAuth access token
  longer) to show which servers to list — this was recognized as
  unnecessary mid-session and removed before ever being wired up.
  `/setup` already records `guilds.owner_id`; `/dashboard` lists every
  guild where that column matches the logged-in user's ID
  (`storage::models::get_guilds_owned_by`), and `/dashboard/{guild_id}`
  independently re-checks the same match before rendering anything. This
  also keeps the "don't retain OAuth access tokens longer than needed"
  principle intact — the token is used once, for `/users/@me`, and
  discarded.
- Session cookie (`bw_dashboard`) is set and read by hand in
  `web/routes.rs` (`HeaderMap` / `header::SET_COOKIE` / `header::COOKIE`)
  rather than adding a cookie-parsing crate — one cookie, one name, a
  `.split(';')` + `.split_once('=')` is the whole implementation.
  `HttpOnly` always; `Secure` whenever `PUBLIC_BASE_URL` starts with
  `https://`; `SameSite=Lax`; 24-hour `Max-Age` matching the session
  store's own expiry.
- `/dashboard/{guild_id}` renders the same `permissions::check()` +
  `dangerous_role_ids()`-based score as `/security-score` (via the new
  shared `PermissionFindings::score()`, see Stage 8 above) plus the 10
  most recent `security_events` rows for that guild
  (`models::get_recent_security_events`).

## Verification performed

- `cargo build`, `cargo clippy --all-targets`, `cargo fmt` — all clean
  after every stage and again after the final Stage 10 wiring.
- Live smoke test (`RUST_LOG=info cargo run`, timed and killed) with the
  real `.env` (no `DISCORD_CLIENT_SECRET`): gateway connects, commands
  register, web server correctly stays disabled with a warning — confirms
  Stages 7–9's gateway-event handling and new commands don't break
  startup even when the web server is off.
- A second live smoke test with `DISCORD_CLIENT_SECRET` /
  `PUBLIC_BASE_URL` / `WEB_BIND_ADDR` temporarily set to local dummy
  values (real `.env` read first, edited only for the run, and restored
  to its exact prior blank state immediately after — the real bot token
  in that file was never modified or printed) confirmed, via `curl`:
  - `GET /dashboard` with no session cookie → `303` to `/dashboard/login`.
  - `GET /dashboard/login` → `303` to Discord's OAuth authorize endpoint
    with `scope=identify`, the correct `/dashboard/callback` redirect
    URI, and a `state` token present.
  - `GET /dashboard/{guild_id}` with no session cookie → `303` to
    `/dashboard/login` (the session check runs before the ownership
    check, so an anonymous request never gets to leak whether a given
    guild ID exists in the database).
  - `GET /dashboard/callback` with no `code`/`state` query params →
    renders the existing error page, not a panic.
  - `GET /dashboard` with a garbage (never-issued) session cookie value
    → treated as logged-out, `303` to `/dashboard/login` — no panic on
    an invalid token.
  - `GET /dashboard/not-a-number` → `400` (axum's own path-extractor
    rejection for a non-`u64` segment), not a panic or a 500.
  - `GET /dashboard/login` response carries no `Set-Cookie` header — only
    `/dashboard/callback`, after a real login completes, should ever set
    one.
- **Not yet done:** an actual end-to-end OAuth click-through for either
  the anti-raid/anti-nuke detectors (both need either multiple real
  Discord accounts acting in a burst, or a single test account performing
  genuinely destructive-looking actions — neither is safe or easy to
  fully simulate solo) or the dashboard login (needs a real Discord
  account completing Discord's consent screen, same caveat Stage 5/6 left
  open for member verification). This is the "start testing" phase the
  user asked for next, not a gap in this stage's own work.

## Post-completion setup panel update

After the Stage 7-10 pass, `/setup` was redesigned from an option-heavy
slash command into an ephemeral interactive panel:

- `/setup` now has no slash-command options. It opens an admin-only embed
  with a text-channel selector, Verified-role selector, Quarantine-role
  selector, and buttons for **Create Defaults**, **Support Join**, and
  **Quick Check**.
- A new server starts with the selectors disabled. **Create Defaults**
  creates or reuses the original `#blackwall-logs`, `Verified`, and
  `Quarantine` defaults, saves the configuration, and then enables the
  selectors. This action uses a deferred component response because it can
  create several Discord resources.
- Subsequent selector choices update only that one stored field, so changing
  a log channel never replaces the configured roles or feature toggles.
  **Support Join** keeps the Stage 6 opt-in semantics, and **Quick Check**
  reruns the focused permission check without changing configuration.
- `interactions.rs` now routes `MessageComponent` interactions whose custom
  IDs begin with `setup:`. Each action re-checks `MANAGE_GUILD` or
  `ADMINISTRATOR`; the panel being ephemeral is a convenience, not the only
  authorization control.
- Two unit tests cover the panel's initialization boundary: selectors remain
  disabled until defaults exist, then enable once a saved configuration is
  present. A real Discord click-through remains part of the next live-test
  phase.

## What's still open

- No `/whitelist` (staff bypass) command — flagged as a gap back in Stage
  3 and still true: no moderation feature (spam, raid, nuke) distinguishes
  staff from regular members yet. Everyone except the guild owner is
  treated identically.
- No `/config` command for toggling `anti_raid_enabled` /
  `anti_nuke_enabled` (or the older `anti_spam_enabled` /
  `anti_scam_enabled`) short of a direct database edit — same open item
  noted in `06_STAGE_6_COMPLETION.md`, now with two more settings that
  need it.
- `/backup` only covers text channels, categories, and non-managed roles
  — no voice channels, no channel permission overwrites, no emoji/
  stickers, no message content. `/restore` is by-name and additive-only
  (see Stage 9 section above) — it does not touch or remove anything that
  already exists, only recreates what's missing.
- The dashboard is read-only. It shows score and recent events but has no
  write actions yet (toggling settings, triggering `/lockdown`, viewing
  backups) — the original vision docs' fuller dashboard scope is not
  fully built, just the read-only slice that was achievable in this
  session's time budget.
- The landing page's "Add bot" button (noted as still-missing in
  `06_STAGE_6_COMPLETION.md`) still doesn't exist.
