# Blackwall

A Discord security bot written in Rust: anti-raid, anti-nuke, anti-spam,
anti-scam, verification, and more — built stage by stage.

This README is written for a first-time Rust developer. It explains what
each file does and how to get the bot running locally.

**For the full design history and remaining plan** — the original brief,
the as-built architecture, a stage-by-stage build log, a detailed
implementation plan for every stage not yet built, and every hard-won
gotcha discovered along the way — see [docs/](docs/README.md). That
document set is written to be detailed enough for an AI or a new
contributor to pick this project up cold and continue it faithfully; this
README stays focused on "how do I run this right now."

## What exists right now

**Stage 1:**
- The bot connects to Discord's gateway (a permanent websocket connection)
  and logs every message it can see to the terminal.
- It registers slash commands and can reply to them — `/ping` proves the
  whole pipeline works end to end.

**Stage 2:**
- Every message is checked against a list of known scam/phishing patterns
  (fake Nitro gifts, Steam scam links, free Robux scams, token grabbers,
  fake giveaway wording) using [Aho-Corasick](https://en.wikipedia.org/wiki/Aho%E2%80%93Corasick_algorithm),
  which matches all patterns in a single fast pass over the text. The
  pattern list is compiled into a matcher **once at startup** — never
  rebuilt per-message.
- A message that matches gets deleted immediately.

**Stage 4:**
- Blackwall now has a real SQLite database, with **one row per server**.
  Nothing is shared globally between servers anymore.
- `/setup` opens an ephemeral setup panel for admins. Its **Create Defaults**
  button resolves (or creates) a log channel plus Verified/Quarantine roles;
  its Discord-native dropdowns let an admin change those choices later, and
  its **Quick Check** button surfaces obvious permission risks. Requires the
  **Manage Server** permission to open or use.
- The scam filter's log embeds now go to *that server's* configured log
  channel, looked up from the database — not one bot-wide channel.

(Stage 3, anti-spam, was deliberately done *after* Stage 4 instead of
before it — the bot originally had one global, env-var-configured log
channel, which is wrong for a bot meant to run on many servers at once.
Stage 4 fixed that foundation first, so anti-spam (which also needs
per-server settings, e.g. to be toggled off) wouldn't get built on top of
a broken assumption.)

**Stage 3:**
- Every message is checked for spam patterns that need more than one
  message to detect: **message bursts** (6+ messages in 10 seconds),
  **copy-pasted repeats** (the same message 3+ times in a row), and
  **mention spam** (5+ user mentions in one message). Per-user, per-guild
  history is tracked in a `DashMap` built once at startup — same idea as
  the scam matcher.
- A message that trips a rule gets deleted, and its author is timed out
  (Discord's built-in "communication disabled until" — 10 minutes for now)
  — no separate mute role needed.
- Both this and the scam filter now respect each server's
  `anti_spam_enabled` / `anti_scam_enabled` toggles from the database
  (defaulting to on for a server that hasn't run `/setup` yet). Those
  columns existed since Stage 4 but nothing read them until now.

The server **owner** is never timed out — Discord doesn't allow it for
anyone, regardless of the bot's permissions, so Blackwall checks this
first (a local database lookup, no wasted API call) and still deletes the
message, but logs "timeout skipped" instead of attempting a call that
would always fail. Found via live testing: the bot's first timeout
attempts against an owner account came back as a confusing "Missing
Permissions" error that looked like a bot-permission problem but wasn't.

Not yet built: emoji/attachment/sticker/invite-link spam volume, Zalgo
text detection, and a **staff**/whitelist bypass — unlike the owner case
above, a regular member with Manage Messages can still be timed out like
anyone else today. The planned `/whitelist` command is the natural fix
for that, not something worth special-casing here first. No one is
banned or kicked yet, and there's no support-server auto-join yet.
`/setup`'s permission checks are a small, high-signal sample — not the
full audit `/security-score` (a later stage) will do.

**Stage 5:**
- Blackwall now has a built-in verification website served by the same
  Rust binary. When `DISCORD_CLIENT_SECRET` is configured, the bot starts
  an `axum` server with `/`, `/verify`, `/callback`, and `/success`.
- `/verify-panel` posts a public Verify button in the current Discord
  channel. Users click it, approve Discord's `identify` OAuth scope, and
  Blackwall grants the server's configured Verified role.
- OAuth state tokens are one-time use and expire after 10 minutes. User
  OAuth access tokens are used only inside the callback request and are
  not stored.
- Successful verifications are saved in `verified_users`. Scam deletions,
  spam timeouts, and verification successes are also recorded in
  `security_events` for the later dashboard.

The full OAuth click-through still needs a real browser session with a
matching redirect URI configured in the Discord Developer Portal.

**Stage 6:**
- The panel's **Support Join** button turns the feature on or off per
  server. It starts off and remains unavailable until this Blackwall
  instance has a `SUPPORT_GUILD_ID` configured.
- If a server has it on **and** this Blackwall instance has
  `SUPPORT_GUILD_ID` configured, the verify page requests the extra
  `guilds.join` OAuth scope and — in plain, visible body text, never
  fine print — discloses that continuing may add you to Blackwall's
  official support server. If either condition isn't met, nothing
  changes: just the `identify` scope, no mention of a support server.
- After the primary Verified-role grant succeeds, the support-server add
  is attempted as a best-effort extra step — its failure never undoes or
  blocks the verification that already happened. Both outcomes get a log
  embed and a `security_events` row.
- New pages: `/privacy`, `/terms`, `/support` (with a manual join link if
  `SUPPORT_SERVER_INVITE_URL` is set). Every page's footer now links to
  all three.

**Stage 7:**
- Blackwall now watches member joins (needs the **Server Members
  Intent** privileged toggle — see setup step 2 below) and detects raids:
  either 10+ joins within 60 seconds, or 5+ joins in that window that are
  individually suspicious (no avatar and/or an account under 7 days old).
- On detection: every text channel gets locked (same mechanism as
  `/lockdown` below — snapshotting each channel's exact prior `@everyone`
  overwrite first, so `/unlockdown` restores it exactly rather than just
  clearing whatever was added), the individually-suspicious joiners from
  that window get a 24-hour timeout (longer than the 10-minute spam
  timeout — a raid is a much stronger signal), and staff get a log embed
  with a timeline of the joins that triggered it. Non-suspicious joiners
  caught in the same burst are *not* timed out — only volume alone
  triggering the alarm shouldn't punish someone who did nothing
  individually suspicious.
- New commands: `/lockdown` (locks every text channel manually) and
  `/unlockdown` (restores exactly what `/lockdown` changed, using the
  same snapshot). Both require **Manage Server** and log to the
  configured log channel.
- The server owner is skipped for raid-response timeouts too, same
  reasoning as the Stage 3 fix above.

**Stage 8:**
- Blackwall now watches the **audit log** (needs `VIEW_AUDIT_LOG`
  in-server — the `GUILD_MODERATION` gateway intent that carries these
  events is *not* privileged, so there's no extra Developer Portal toggle
  for it) and detects nuke attempts: 3+ "dangerous" actions (channel/role
  deletes, bans, kicks, webhook creation, role/permission edits, guild
  settings edits, or a new bot being added) by the **same actor** within
  30 seconds.
- On detection: the actor has every Administrator/Manage-Guild/
  Manage-Roles/Manage-Channels/Manage-Webhooks role stripped, gets
  quarantined (if a Quarantine role is configured) or timed out as a
  fallback, every text channel gets locked down (same snapshot/restore
  mechanism as `/lockdown`), the configured server owner gets a DM, and
  staff get a log embed. The server owner is exempt from all of this —
  same reasoning as the spam/raid fixes above, since Discord would refuse
  the API calls against them anyway.
- New command: `/security-score` — reruns `/setup`'s permission checks
  (plus a new one: is the bot's own role positioned *above* every
  dangerous role, since anti-nuke's role-stripping and quarantine can't
  work otherwise) and turns the findings into a 0-100 score (-30 per
  critical finding, -10 per medium finding), shown with the full list of
  what's wrong. Requires **Manage Server**, ephemeral reply.

**Stage 9:**
- New commands: `/backup` snapshots every non-managed role (name, color,
  permissions, hoist/mentionable, position) and every text
  channel/category (name, position, parent) as one JSON blob in the
  database; `/restore` reads the latest backup and recreates anything
  currently missing **by name** — new roles/channels get fresh Discord
  IDs, so anything that referenced the old IDs (permission overwrites on
  other channels, bot configuration elsewhere) won't automatically point
  at the restored ones. `/restore`'s reply says this explicitly rather
  than implying a perfect rollback. Both commands require **Manage
  Server**.

**Stage 10:**
- Blackwall now has an owner-only web dashboard at `/dashboard`, using the
  same `axum` server as verification (still gated on
  `DISCORD_CLIENT_SECRET` being set). Logging in uses a **separate**
  OAuth flow from member verification (`/dashboard/login`,
  `identify`-only scope) with its own longer-lived (24 hour) session
  cookie — `HttpOnly`, `SameSite=Lax`, and `Secure` whenever
  `PUBLIC_BASE_URL` is `https://`.
- Access control needs no new database table and no extra OAuth scope:
  `/dashboard` lists every server where the logged-in Discord user ID
  matches that server's `owner_id` (already recorded by `/setup`), and
  `/dashboard/{guild_id}` re-checks that same match before showing
  anything. Blackwall never asks Discord which servers a user is in.
- The per-server page (`/dashboard/{guild_id}`) shows the same security
  score and findings as `/security-score`, plus the 10 most recent
  `security_events` rows for that server.

**Beyond the original 10 stages — alt-account detection, without IP addresses:**

An IP-matching alt-account checker was considered and deliberately ruled
out: public IPs are frequently shared by unrelated people (mobile carrier
NAT, campus/office networks, VPNs), so a raw IP match is a weak signal
that would block real, innocent users, and cross-referencing IPs across
different servers raises real privacy questions on top of that. Two
zero-new-data-collection alternatives were built instead:

- `/setup` and `/security-score` now flag it (medium severity) when a
  server's **Verification Level** isn't set to the highest tier — that
  tier requires a verified phone number to join, which is Discord's own
  strongest deterrent against throwaway alt accounts, and needs no new
  data collection on Blackwall's side at all.
- A new **ban/kick-evasion flag**: Blackwall now watches for
  `MemberBanAdd`/`MemberKick` audit-log entries (regardless of whether
  they trip the anti-nuke threshold) and, for 30 minutes afterward, flags
  any new join that also looks individually suspicious (new account, no
  avatar — the same heuristic anti-raid already uses). This is a *flag,
  not a block* — it logs a neutral-colored "Possible ban/kick evasion"
  embed and says plainly that it's a timing correlation, not proof, since
  Blackwall has no way to actually link a new Discord account to a
  removed one without IP or device data.

## One-time setup

### 1. Create a Discord application + bot

1. Go to <https://discord.com/developers/applications> and click **New
   Application**. Name it whatever you like (e.g. "Blackwall Dev").
2. Open the **Bot** tab.
   - Click **Reset Token** and copy the token somewhere safe — you'll need
     it in a moment. Discord only shows it once.
   - Under **Privileged Gateway Intents**, turn on **Message Content
     Intent** and **Server Members Intent**. The bot cannot read message
     text, or see who joins, without these switches — Discord refuses the
     entire gateway connection otherwise (not just the affected feature).
3. Open the **OAuth2 -> URL Generator** tab.
   - Under **Scopes**, check `bot` and `applications.commands`.
   - Under **Bot Permissions**, check at least: Manage Roles, Manage
     Channels (also used by `/lockdown`'s channel overwrites), Send
     Messages, Manage Messages (needed to delete other people's
     messages), Read Message History, **Timeout Members** (needed for
     the anti-spam/anti-raid/anti-nuke timeouts), **Kick Members** and
     **Ban Members** (anti-nuke may need to act on these), and **View
     Audit Log** (needed for anti-nuke to see who did what).
   - Copy the generated URL, open it in your browser, and invite the bot
     to a test server you own.

   One thing to double check after inviting: the bot's own role must sit
   **above** any role it needs to create/manage/timeout in the role list
   (Server Settings -> Roles), or Discord will reject those requests. This
   is usually only an issue if you drag the bot's role very low.
4. Only if you want the verification website (Stage 5) working end to
   end — open the **OAuth2 -> General** tab.
   - Click **Reset Secret** (or **Copy** if one's already set) to get the
     **Client Secret** — this goes in `DISCORD_CLIENT_SECRET` below. Treat
     it like a password, same as the bot token.
   - Under **Redirects**, add `http://localhost:8080/callback` (or
     `{your PUBLIC_BASE_URL}/callback` if you changed that default).
     Discord rejects the OAuth flow with an "Invalid redirect_uri" error
     if this isn't registered here first, and it must match exactly
     (scheme, host, port, path).

### 2. Configure your `.env` file

Copy `.env.example` to `.env` and fill in:

```
DISCORD_TOKEN=your-bot-token-from-above
TEST_GUILD_ID=your-test-server-id
DATABASE_URL=
DISCORD_CLIENT_SECRET=
PUBLIC_BASE_URL=
WEB_BIND_ADDR=
SUPPORT_GUILD_ID=
SUPPORT_SERVER_INVITE_URL=
```

`TEST_GUILD_ID` is optional but recommended: it makes slash commands show
up in your test server within seconds. Without it, Discord can take up to
an hour to roll a new global command out everywhere.

`DATABASE_URL` is **required**: Supabase's *direct* Postgres connection
string (Project Settings -> Database -> Connection string -> URI in the
Supabase dashboard) — not the pooler connection, and not the
`service_role` key. Blackwall is a single persistent process, not a swarm
of short-lived serverless functions, so there's nothing for a connection
pooler to help with, and Supabase's default pooler (PgBouncer in
transaction mode) doesn't support the prepared statements `sqlx` uses by
default. Run `blackwallsite/supabase/schema.sql` once in the Supabase SQL
editor before starting the bot — this is the same database the owner
dashboard website reads from, so the two always agree.

`DISCORD_CLIENT_SECRET` is optional for the bot overall, but required for
the verification website. If it is blank, Blackwall logs a warning and
runs the Discord bot without the web verification flow.

`PUBLIC_BASE_URL` defaults to `http://localhost:8080`. This is the URL
put into `/verify-panel` links and the OAuth `redirect_uri`; when you
deploy, it must match the redirect URI configured in Discord's Developer
Portal.

`WEB_BIND_ADDR` defaults to `127.0.0.1:8080`. This is where the built-in
web server listens locally.

`SUPPORT_GUILD_ID` is optional. Set it to your own community/support
server's ID to make the support-server-join feature available at all —
individual servers still need to opt in with the **Support Join** button
in their `/setup` panel on top of this.

`SUPPORT_SERVER_INVITE_URL` is optional. A plain `https://discord.gg/...`
link shown on the `/support` page as a manual fallback. Separate from
`SUPPORT_GUILD_ID` (that one's used for the automatic OAuth join; this
one's just a link for humans).

To find a server's ID: in Discord, go to **User Settings -> Advanced** and
turn on **Developer Mode**, then right-click the server icon and choose
**Copy Server ID**.

`.env` is listed in `.gitignore` — it will never be committed. Never share
your real token with anyone or paste it into chat, logs, or screenshots.

### 3. Run it

```
cargo run
```

You should see log lines like `Blackwall is online`. Try:
- `/ping` — should reply "Pong!" within a second.
- `/setup` opens an ephemeral setup panel. Select the log channel and
  security roles from the dropdowns; use **Create Defaults** to provision
  them on a new server, and **Quick Check** to refresh permission findings.
- Posting a message containing a phrase like "free nitro" — it should
  disappear immediately, and a log embed should show up in whatever
  channel `/setup` configured.
- Posting the exact same message 3 times in a row, or sending 6+ messages
  within 10 seconds, or @mentioning 5+ users in one message — any of
  those should get the message deleted and you timed out for 10 minutes
  (use an alt account or ask a friend to test this one — you probably
  don't want to time yourself out on your main).
- In `/setup`, turn **Support Join** on (with `SUPPORT_GUILD_ID` configured)
  and the verify page for that server should now show the
  support-server-join disclosure and request `identify guilds.join`
  instead of just `identify`. Visit `/privacy`, `/terms`, and `/support`
  directly to check the new legal pages render.
- `/lockdown` — should lock every text channel (try sending a message as
  a regular member afterward; it should be blocked) and post a log embed.
  Then `/unlockdown` should restore exactly what it changed.
- Raid detection needs several accounts joining in a burst to trigger
  honestly, which is hard to simulate solo — the safest way to see it
  fire without touching a real community is to watch the log channel
  after asking a few alt accounts (or friends) to join your test server
  within a short window.
- `/security-score` — should reply with a score out of 100 and a
  breakdown of any permission risks found (try it once on a clean test
  server, then again after giving `@everyone` Administrator to see the
  score and critical-findings list change).
- Nuke detection needs 3+ dangerous actions (channel/role deletes, bans,
  kicks, webhook creation, role edits, etc.) by the *same* account within
  30 seconds to trigger honestly — on an alt/test account with elevated
  permissions, try deleting a couple of throwaway channels and roles in
  quick succession and watch for the role-strip/quarantine/lockdown
  response and log embed. Don't try this against your own owner account —
  it's exempt by design, same as spam/raid.
- `/backup` then delete a throwaway role or channel, then `/restore` —
  the missing one should reappear (as a new object with a new ID; the
  reply says so).
- With `DISCORD_CLIENT_SECRET` and `PUBLIC_BASE_URL` configured, visit
  `/dashboard` in a browser — you should be redirected to
  `/dashboard/login`, through Discord's OAuth screen, and back to a page
  listing every server where you're recorded as the owner (from
  `/setup`). Click through to one to see its security score and recent
  security events.
- Lower your test server's Verification Level below "Highest" — the next
  `/setup` or `/security-score` run should flag it as a medium finding.
- Ban or kick an alt/test account, then have a second alt/test account
  (with no avatar, or a freshly-created account) join within 30 minutes —
  you should get a neutral-colored "Possible ban/kick evasion" log embed,
  with no timeout or block applied to the new joiner.

## Project layout

```
src/
  main.rs            Startup: load config, connect to the database and
                       Discord, run forever.
  config.rs           Reads every environment variable Blackwall uses
                       (Discord token, database path, OAuth/web config,
                       support-server config) into one `Config` struct.
  state.rs            Shared data every part of the bot can access (HTTP
                       client, application ID, compiled scam matcher,
                       database pool, per-user spam/join/nuke trackers,
                       OAuth client, verification + dashboard sessions).
  gateway.rs           Connects to Discord's gateway and reacts to events
                       (new messages, joins, audit log entries, slash
                       commands, etc.) forever.
  discord/
    mod.rs             Just wires the submodules below together.
    http.rs            Builds the REST API client used to send requests to
                        Discord (as opposed to the gateway, which only
                        *receives* events).
    commands.rs         Defines the bot's slash commands and registers them
                        with Discord on startup.
    interactions.rs      Routes slash commands and setup-panel component
                          interactions to their handlers.
    setup.rs             The `/setup` panel: initializes defaults, handles
                         channel/role dropdowns and buttons, saves config,
                         and runs the quick permission check.
    verify_panel.rs       The `/verify-panel` command: posts the public
                          Verify button that opens the website.
    lockdown.rs           The `/lockdown` and `/unlockdown` commands.
    security_score.rs      The `/security-score` command: permission
                           findings plus a bot-role-hierarchy check, turned
                           into a 0-100 score.
    backup.rs              The `/backup` and `/restore` commands: snapshot
                           and best-effort recreate roles/channels by name.
    embeds.rs             Builds the embeds sent to Discord (moderation
                          logs, the `/setup` panel, raid timelines, nuke
                          alerts).
  moderation/
    mod.rs             Just wires the submodules below together.
    scam.rs             The scam/phishing phrase list, the Aho-Corasick
                        matcher built from it, and the check function that
                        tests a message against it.
    spam.rs             Per-user, per-guild message history and the rules
                        (burst / repeat / mention) that check it.
    raid.rs              Per-guild join history and the rules (burst /
                        suspicious accounts) that check it.
    nuke.rs               Per-(guild, actor) dangerous audit-log-action
                        history and the burst rule that checks it.
    evasion.rs             Tracks the most recent ban/kick per guild, so a
                        suspicious new join shortly after can be flagged
                        (never auto-blocked) as a possible evasion attempt.
    permissions.rs        Shared permission-risk checks (`@everyone`
                        Administrator, admin counts, dangerous role IDs,
                        Verification Level) used by `/setup`,
                        `/security-score`, and the dashboard.
  actions/
    lockdown.rs           Locks/unlocks every text channel, with
                          snapshot/restore of each channel's exact prior
                          `@everyone` overwrite. Called from `/lockdown`,
                          the automatic raid response, and the automatic
                          nuke response.
  utils/
    ids.rs               Decodes a Discord snowflake ID's embedded
                        creation timestamp (used by anti-raid).
  storage/
    mod.rs             Just wires the submodules below together.
    database.rs          Connects to the shared Supabase Postgres
                          database. Doesn't create tables — the schema's
                          one authoritative source is
                          `blackwallsite/supabase/schema.sql`, run once by
                          hand in Supabase's SQL editor, since the same
                          database is also read by the dashboard website.
    models.rs            The `GuildConfig` / `GuildSettings` types and the
                          queries that read and write them, plus
                          `verified_users` / `security_events` /
                          `lockdown_snapshots` / `backups` /
                          `security_scores` reads and writes, and the
                          dashboard's `get_guilds_owned_by` lookup.
    cache.rs              `SettingsCache`: an in-memory cache over
                          `guild_settings` so the message-handling hot
                          path never makes a network round-trip to
                          Supabase just to check a few booleans.
  verification/
    sessions.rs          One-time OAuth state tokens for member
                        verification.
    dashboard.rs          Longer-lived OAuth sessions and login-state
                          tokens for the owner dashboard — a separate
                          store from `sessions.rs` on purpose.
    oauth.rs             Discord OAuth URLs (member verification and
                        dashboard login, kept as separate functions), token
                        exchange, and user fetch.
    roles.rs             Grants the Verified role after OAuth succeeds.
  web/
    routes.rs            The axum routes: landing, verify, callback,
                         success, privacy, terms, support, plus the
                         dashboard's login/callback/list/detail routes
                         (manual `HttpOnly` cookie handling, no cookie
                         crate). Also decides whether to offer
                         support-server join and attempts it (best-effort)
                         after verification succeeds.
    templates.rs         Plain Rust functions returning the website HTML,
                         including the privacy/terms/support pages and the
                         dashboard's server-list/server-detail pages.
```

### Why does `main.rs` install a crypto provider before anything else?

```rust
rustls::crypto::ring::default_provider()
    .install_default()
    .expect(...);
```

This looks unrelated to Discord, but it isn't optional. The gateway
connection and any HTTPS request are encrypted using `rustls`, and modern
`rustls` refuses to guess which cryptography backend to use (there are a
couple of interchangeable options) — it must be told explicitly, once,
before the first connection. Skip this and the bot panics the instant it
tries to open a connection. This is a one-time setup step, not something
you'll need to touch again.

### Why is the gateway separate from HTTP?

Discord bots use two different connections:

- **Gateway** (`gateway.rs`) — a websocket that stays open and pushes
  events to us in real time: someone sent a message, someone joined,
  someone ran a slash command.
- **HTTP** (`discord/http.rs`) — regular request/response calls we make
  *to* Discord: send this message, ban this user, register these commands.

You listen on the gateway and act via HTTP.

### Why SQLite, and why store IDs as text?

SQLite is a single file, no server to install or manage — the right choice
for a project this size (the tech plan calls for moving to PostgreSQL
later only if it's ever actually needed). Discord IDs ("snowflakes") are
stored as text rather than numbers because they're 64-bit values that
could in principle exceed what SQLite's signed integer type can hold —
storing them as text sidesteps that question entirely, at the small cost
of a `.to_string()` / `.parse()` at the boundary (see `storage/models.rs`).

## What's next

All 10 planned stages are now built. What's left is testing on a real
server and, likely, a `/whitelist` (staff bypass) command — noted as a
gap back in Stage 3 and still open, since no moderation feature
distinguishes staff from regular members yet. See
[docs/](docs/README.md) for the fuller design history and any newer
notes on what's found during testing.

Blackwall runs as one binary (bot + web server), deployed on a VPS or
similar host that supports long-running processes — not a serverless
platform like Vercel, since the gateway connection and in-memory OAuth/
dashboard session stores both need a process that stays alive
continuously.
