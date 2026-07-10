# Blackwall — Stage 6 completion note

Records the Stage 6 implementation (support-server join disclosure +
legal pages), done directly in this session rather than handed off. Also
records a hosting-architecture question that came up mid-stage and how it
was resolved, since it affects where this whole project gets deployed.

> Later setup UX update: `/setup` no longer exposes the historical
> `support_server_join` slash-command option described below. It now opens
> an ephemeral panel, where the **Support Join** button controls the same
> per-guild setting. The OAuth behavior documented here is unchanged.

## Hosting clarification (came up before this stage started)

The user asked about hosting the website on Vercel. Vercel only runs
serverless functions and static sites — it cannot run Blackwall's
`axum` web server as built, because that server shares in-memory state
(the `SqlitePool`, the OAuth `SessionStore`) with the same process running
the Discord gateway connection, and both need to stay alive continuously.
Splitting the website onto Vercel would mean rewriting it as a separate
Next.js app talking to the bot over a new internal API — a real
architecture change, not a config swap.

Resolved: the user has an existing VPS already running Rust Discord bots.
**Decision: keep the single-binary architecture exactly as built in
Stages 1–5, deploy that one binary on the VPS.** No rework happened or is
planned as a result of this — this note exists so the reasoning isn't
lost if the question comes up again later (e.g. when Stage 10's dashboard
adds a second, longer-lived kind of web session).

## What changed (Stage 6 itself)

- Config gained two new optional variables:
  - `SUPPORT_GUILD_ID` — the bot's own support/community server. Used
    only for the OAuth `guilds.join` add-member API call.
  - `SUPPORT_SERVER_INVITE_URL` — a plain public invite link, unrelated
    to the OAuth flow, shown on `/support` and usable as a manual-join
    fallback. Kept as a separate variable from `SUPPORT_GUILD_ID`
    deliberately: a guild ID alone can't be turned into a working invite
    link (that needs an actual invite code), so the two serve different
    purposes and shouldn't be conflated.
- `guild_settings.support_join_enabled` (existing column since Stage 4,
  never read until now) is now:
  - Read by `web/routes.rs` before building the OAuth URL and rendering
    the verify page.
  - Writable via a new `/setup` option, `support_server_join` (boolean,
    optional). Leaving the option unset never changes the existing value
    — re-running `/setup` for an unrelated reason (e.g. changing the log
    channel) can't silently flip this back off. Explicitly requesting
    `true` when `SUPPORT_GUILD_ID` isn't configured bot-wide is rejected
    with a warning in the summary embed rather than silently accepted (or
    silently ignored) — the admin needs to know the request had no effect.
  - `/setup`'s summary embed gained a "Support-server join" field showing
    the current on/off state and, when off, how to turn it on.
- The verify page (`GET /verify`) now checks **both** conditions —
  `SUPPORT_GUILD_ID` configured *and* that specific guild's
  `support_join_enabled` — before requesting the extra `guilds.join`
  scope or showing the disclosure sentence. Verified directly (not just
  by reading the code): with both conditions true, the generated OAuth
  URL is `scope=identify+guilds.join` and the disclosure sentence renders;
  with either condition false, it's `scope=identify` and no disclosure
  text appears.
- `/callback` now attempts the support-server join as a best-effort step
  **after** the primary Verified-role grant already succeeded — using
  `twilight_http::Client::add_guild_member(support_guild_id, user_id,
  access_token)`, which requires the user's OAuth token to include
  `guilds.join` (only true when the above two conditions held). Success or
  failure never changes the final redirect — verification already
  succeeded and is not undone. Both outcomes get:
  - A `security_events` row (`support_server_join_success` /
    `support_server_join_failed`).
  - A Discord log embed (`embeds::support_join_result`) to the guild's log
    channel, matching the spec's "log OAuth support-server join
    success/fail" requirement.
  - The final redirect carries `?support_joined=true|false` (or nothing,
    if the feature wasn't offered for that session) so the success page
    can honestly report what happened, rather than always showing the
    same generic message regardless of outcome.
- New pages: `GET /privacy`, `GET /terms`, `GET /support`. Content matches
  what's actually collected (see `storage/database.rs`'s schema) rather
  than generic boilerplate — e.g. the privacy page explicitly says there's
  no automated deletion process yet, rather than implying one exists.
- The shared page `layout()` footer now links to all three new pages on
  every page, not just the ones that reference them contextually. The
  landing page gained a "Support Server" button linking to `/support`
  (which itself has the actual join link, conditional on
  `SUPPORT_SERVER_INVITE_URL` being set) — the original spec's landing
  page section asked for this button; it hadn't been added in Stage 5.

## Verification performed

- `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` — all
  clean.
- Smoke test with no `DISCORD_CLIENT_SECRET` (real `.env`, unchanged):
  gateway connects, `/setup` (now with 4 options) registers, web server
  correctly stays disabled with a warning.
- Smoke test with a dummy client secret + `SUPPORT_GUILD_ID` +
  `SUPPORT_SERVER_INVITE_URL` set: `/`, `/privacy`, `/terms`, `/support`
  all returned 200; `/support`'s rendered join link matched the configured
  invite URL exactly.
- Directly verified the two-condition gate on `/verify`'s OAuth scope by
  writing a `guild_settings` row with `support_join_enabled = 1` straight
  into the running SQLite database (via Python's `sqlite3` module — no
  `sqlite3` CLI was available) while the bot was running, then re-fetching
  `/verify?guild_id=...` and confirming the scope changed from `identify`
  to `identify+guilds.join` (`+` is the query-string encoding of the space
  between scopes) and the disclosure sentence appeared. The test row was
  deleted afterward.
- Did **not** verify the actual `add_guild_member` call against a real
  Discord support server (would need a second real Discord account
  clicking through the flow with `guilds.join` actually granted) — this
  needs a human test, same caveat as Stage 5's OAuth click-through.

## What's still open

- The landing page's "Add bot" button (from the original spec's landing
  page section) still doesn't exist — it needs a constructed Discord bot
  invite URL (scopes + permission integer), which is a small, separate,
  not-yet-built piece of website work, not something this stage's scope
  ("support-server join disclosure + legal pages") covered.
- No `/config` command exists yet for toggling `anti_spam_enabled` /
  `anti_scam_enabled` / etc. the same way `/setup`'s new option toggles
  `support_join_enabled` — those still default on with no way to turn
  them off short of a database edit. `/config` remains unscoped in detail
  (see `03_ROADMAP.md`'s Stage 10 note about sharing one settings-write
  function between it and the future dashboard) — worth scoping properly
  whenever it's actually needed rather than bolting one setting at a time
  onto `/setup` indefinitely.
