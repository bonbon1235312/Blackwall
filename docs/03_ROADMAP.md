# Blackwall — Roadmap (Stages 5–10, detailed implementation plans)

Each section below is written to the level of detail needed to implement
that stage without re-deriving the design from scratch — exact file
trees, exact API calls (verified against the installed crate source where
noted), exact database schema, exact env vars, and the specific gotchas
already known to apply. Follow `01_ARCHITECTURE.md`'s established
patterns (build-once detectors in `AppState`, pure `check()` + `gateway.rs`
action functions, fail-loud-at-startup vs. fail-soft-at-runtime, ephemeral
admin replies, per-guild everything, honest "active now vs. configured
for later" summaries) unless a stage-specific reason to deviate is called
out explicitly.

**Before writing any other Stage 5 code, resolve the open question about
`reqwest`/`rustls`'s crypto backend described in `02_PROGRESS_LOG.md`'s
Stage 5 section and "Step 0" below.** Status as of this writing: `axum`,
`reqwest` (with its default `aws-lc-rs` backend), and `rand` have been
added to `Cargo.toml`, and both `cargo build` and a full smoke test
(gateway connects, "Blackwall is online" logs) **already succeed** in this
state — because nothing in the code has actually used `reqwest` yet, and
the project's existing `rustls::crypto::ring::default_provider()
.install_default()` call in `main()` already resolves the process-wide
provider before that point. Whether this remains fine once `reqwest` is
actually used for real requests is the one open question — Step 0 gives a
five-minute way to test it directly before deciding whether the bigger
"unify on `aws-lc-rs`" fix below is even necessary.

**Cross-cutting retrofit, do this as soon as Stage 5 or 6 lands, don't
wait for Stage 10:** every moderation action (scam deletion, spam timeout,
and everything added from raid/nuke onward) should also write a row to a
`security_events` table (schema below, under Stage 10, since that's where
it's listed in the original spec) — not just send a Discord log embed.
The dashboard (Stage 10) needs this data, and retrofitting it onto every
past action right before Stage 10 is far more error-prone than adding one
`record_security_event(...)` call alongside each action as it's built.
Recommended: create the table and the helper function as part of Stage 5
or 6 (whichever lands first), and call it from `handle_scam_message`/
`handle_spam_violation` retroactively at the same time, then keep calling
it from every new action going forward.

---

## Stage 5 — Verification website + OAuth `identify`

### Step 0: confirm whether there's actually a crypto-backend conflict, then fix it if so

**Do this first, and it only takes a few minutes:** add one throwaway call
in `main()` — right after the existing
`rustls::crypto::ring::default_provider().install_default()` line —
that builds a `reqwest::Client` and issues one real `GET` request to any
HTTPS URL (e.g. `https://discord.com`), and log/print whether it succeeds
or panics. `Cargo.lock` already has both `ring` and `aws-lc-rs`/
`aws-lc-sys` present simultaneously at this point (confirmed via `grep -n
'^name = "ring"$\|^name = "aws-lc-rs"$\|^name = "aws-lc-sys"$'
Cargo.lock`), and the bot's *existing* gateway/HTTP functionality already
tolerates that combination fine (verified: `cargo build` and a full smoke
test both succeed as of `reqwest` being added-but-unused) — precisely
because the process-wide default was already installed as `ring` before
either backend's code paths are exercised. The only genuinely unknown
thing is whether `reqwest`'s own TLS setup respects that same process
default or insists on its own. This one throwaway request answers that
directly, empirically, in less time than reasoning about it would take —
see `04_GOTCHAS_AND_LEARNINGS.md` #3 for the full reasoning and why this
project is treating "test it" as strictly better than "assume it."

**If the throwaway request succeeds:** delete it, add nothing further,
proceed with the rest of Stage 5 using `ring` unchanged. No further action
needed.

**If the throwaway request panics**, two remediation options, in
preference order:

1. **Recommended: standardize the whole project on `aws-lc-rs` instead of
   `ring`.** `reqwest`'s `rustls` feature pulls in `aws-lc-rs` by default
   on this crate version (`reqwest 0.13.4`) — fighting that default is
   more effort than just matching it. Concretely:
   - Change the project's own `rustls` dependency features from
     `["ring", "std", "tls12", "logging"]` to
     `["aws_lc_rs", "std", "tls12", "logging"]` (drop `ring`, add
     `aws_lc_rs` — note the underscore, matching the feature name seen in
     the `cargo add rustls` output during Stage 4).
   - Change `main.rs`'s startup line from
     `rustls::crypto::ring::default_provider().install_default()...` to
     `rustls::crypto::aws_lc_rs::default_provider().install_default()...`
     (verify this function exists at
     `rustls-<version>/src/crypto/aws_lc_rs/mod.rs::default_provider` the
     same way `ring`'s equivalent was verified in Stage 4 — it was seen
     directly in that file during this session's investigation, so it
     should already be present).
   - `aws-lc-rs` needs a C build toolchain (`cmake` was pulled in as a
     build-dependency, confirming this) — on Windows this generally means
     having a working MSVC toolchain available (which `rustc`'s own
     `windows-msvc` target already requires, so this is very likely
     already satisfied, but if the build fails with a C-compiler-not-found
     error, that's the first thing to check).
   - Re-run the full verification sequence: `cargo build`, `cargo clippy
     --all-targets`, `cargo fmt`, then the **smoke test**
     (`RUST_LOG=info timeout 20 cargo run`, confirming gateway connection
     still succeeds) before writing any other Stage 5 code.
2. **Alternative: find a reqwest feature that selects `ring` specifically**
   and use that instead, keeping the project on `ring`. Check
   `reqwest-0.13.4/Cargo.toml` (once downloaded — `cargo fetch` first) for
   a feature name along these lines (candidates to check for:
   `rustls-tls-ring`, or a `ring` sub-feature reachable by disabling
   `reqwest`'s default `__rustls-aws-lc-rs` internal feature and manually
   enabling an equivalent `__rustls-ring` one — the leading-underscore
   features seen in the `cargo add` output are reqwest's internal feature
   plumbing and may not be meant to be selected directly, so treat this
   path as more fragile than option 1). Only pursue this if there's a
   specific reason to prefer `ring` over `aws-lc-rs` (there isn't one
   identified in this project so far) — otherwise take option 1.

Either way, the **verification step is non-negotiable**: check
`Cargo.lock` afterward for the *absence* of whichever backend wasn't
chosen (grep for `^name = "ring"$` / `^name = "aws-lc-rs"$` /
`^name = "aws-lc-sys"$`), and confirm the smoke test still logs "Blackwall
is online" without a `CryptoProvider` panic.

### New dependencies

- `axum` — already added successfully in Stage 4/5 transition (see
  `02_PROGRESS_LOG.md`). Default features are fine as-is.
- `reqwest` — for the two raw HTTP calls the OAuth flow needs that
  `twilight-http` doesn't cover (it's hardcoded to bot-token-authenticated
  Discord bot-API requests, not arbitrary user-token-authenticated OAuth
  calls): exchanging an authorization code for an access token, and
  fetching the authorized user's identity with that token. Add with
  `--no-default-features --features <resolved per Step 0>,json`.
- `rand` — already added successfully. Use for generating the OAuth
  `state` CSRF token: generate e.g. 32 random bytes via
  `rand::rng().fill_bytes(...)` (verify the exact current API — `rand`
  0.9/0.10 renamed `thread_rng()`-style APIs a few times across versions;
  check `rand-0.10.2/src/lib.rs` for the actual current entry point before
  writing this code, per the "verify against installed source" rule) and
  hex-encode manually (`format!("{:02x}", byte)` in a loop, joined) —
  no need for a `hex` crate dependency for something this small.

### New environment variables (add to `config.rs`)

```
DISCORD_CLIENT_SECRET   required *for the web server to start*, but must
                        NOT be required for the bot overall — see
                        "graceful degradation" below.
PUBLIC_BASE_URL         optional, default "http://localhost:8080". The
                        externally-reachable base URL used to build the
                        OAuth redirect_uri and the /verify link posted by
                        /verify-panel.
WEB_BIND_ADDR           optional, default "127.0.0.1:8080". What the axum
                        server actually binds to — kept separate from
                        PUBLIC_BASE_URL because in a real deployment these
                        can differ (e.g. bound on 0.0.0.0 internally,
                        reachable publicly through a reverse proxy on a
                        different host/port/scheme).
```

Do **not** add a separate `DISCORD_CLIENT_ID` env var — the bot's
Discord application ID (already fetched once at startup into
`AppState.application_id`) *is* the OAuth2 client ID. Reusing it avoids
asking the user to copy the same value into two places.

**Graceful degradation, important:** if `DISCORD_CLIENT_SECRET` is unset,
log a `tracing::warn!` explaining that verification/the web server is
disabled, and simply don't spawn the web server task — the rest of the
bot (gateway, scam/spam detection, `/setup`) must keep working exactly as
before for anyone who hasn't set up OAuth yet. This is the same
"don't break existing behavior while adding new optional behavior"
posture used when `LOG_CHANNEL_ID` was made optional in Stage 2.

### New database table

```sql
CREATE TABLE IF NOT EXISTS verified_users (
    guild_id     TEXT NOT NULL,
    user_id      TEXT NOT NULL,
    verified_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    method       TEXT NOT NULL,
    PRIMARY KEY (guild_id, user_id)
);
```

`method` should be the literal string `"oauth"` for now (a future
`"manual"` or `"captcha"` method isn't in scope yet — don't build
alternatives that don't exist). Add a `storage::models::record_verification
(pool, guild_id, user_id, method)` function (`INSERT OR REPLACE` — a user
re-verifying should just update their timestamp, not error).

### New module tree

```
src/verification/
  mod.rs       Re-exports submodules.
  sessions.rs  SessionStore: CSRF state-token generation + one-time-use
               validation.
  oauth.rs     Building the Discord authorize URL; exchanging a code for
               a token; fetching the authorized user's identity. All via
               `reqwest`, never via `twilight_http` (that client is
               bot-token-only).
  roles.rs     Applying the verified role via the BOT token
               (`twilight_http`) once OAuth has confirmed who the user is
               — this is the one place verification touches
               `twilight_http` instead of `reqwest`.
src/web/
  mod.rs       Re-exports submodules; `pub async fn run(state, bind_addr)`
               builds the axum Router and serves it.
  routes.rs    Route handlers: GET /, GET /verify, GET /callback,
               GET /success (Stage 5); GET /privacy, GET /terms,
               GET /support added in Stage 6.
  templates.rs Plain Rust functions returning `String` HTML. No template
               engine dependency — see `00_VISION.md`'s tech-stack
               rationale. One shared `layout(title, body) -> String`
               wrapping a `<head>` with an inline `<style>` block (dark
               background, one accent color, no external CSS/JS
               framework), plus one function per page.
```

### `verification/sessions.rs` — design

```rust
pub struct PendingVerification {
    pub guild_id: Id<GuildMarker>,
    pub created_at: Instant,
}

#[derive(Default)]
pub struct SessionStore {
    sessions: DashMap<String, PendingVerification>,
}
```

- `create(&self, guild_id) -> String` — generate a random hex token (see
  `rand` note above), insert `PendingVerification { guild_id, created_at:
  Instant::now() }`, return the token. This token is the OAuth `state`
  parameter.
- `take(&self, token: &str) -> Option<PendingVerification>` —
  `self.sessions.remove(token)` (one-time use: consuming it removes it
  immediately, so replaying an old callback URL can't work), then check
  `created_at.elapsed() <= Duration::from_secs(600)` (10-minute expiry) —
  return `None` if expired or not found, `Some` otherwise. **Removing on
  every lookup attempt (even failed/expired ones) is important** — don't
  leave a token that failed validation sitting in the map.
- Same accepted-limitation note as `SpamTracker`/future join-tracker: a
  user who starts `/verify` and never finishes leaves an entry until
  either it's looked up (and found expired) or never looked up again
  (permanent small leak, proportional to abandoned attempts, not swept
  proactively). Acceptable for MVP; a background sweep task would be the
  natural fix if this ever matters in practice.
- Add `sessions: SessionStore` to `AppState`, built once in `main.rs`
  alongside everything else.

### `verification/oauth.rs` — design

```rust
pub fn authorize_url(
    client_id: Id<ApplicationMarker>,
    redirect_uri: &str,
    state_token: &str,
    include_guilds_join: bool,   // false in Stage 5; wired up in Stage 6
) -> String
```
Builds `https://discord.com/api/oauth2/authorize` with query params
`client_id`, `redirect_uri` (URL-encoded), `response_type=code`,
`scope` (`"identify"` or `"identify guilds.join"` — space-separated,
URL-encoded, depending on `include_guilds_join`), `state` (the token from
`SessionStore::create`). Use `reqwest::Url` or manual
`percent-encoding`/`urlencoding` for the query-param encoding — check
what's already transitively available (both `axum` and `reqwest` pull in
`form_urlencoded`/`url` crates transitively) before adding yet another
dependency for this.

```rust
pub struct TokenResponse {
    pub access_token: String,
    // token_type, expires_in, scope, refresh_token also come back but
    // are not needed/stored — refresh behavior is explicitly out of
    // scope per 00_VISION.md's "never store tokens longer than needed"
    // principle. Deserialize only what's used, or use
    // #[serde(default)] / ignore-unknown-fields defaults for the rest.
}

pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: Id<ApplicationMarker>,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
) -> Result<TokenResponse, reqwest::Error>
```
`POST https://discord.com/api/oauth2/token`, form-encoded body
(`grant_type=authorization_code`, `code`, `redirect_uri`, `client_id`,
`client_secret`) — use `reqwest::RequestBuilder::form(&params)` (a
`&[(&str, &str)]` or a small struct with `#[derive(Serialize)]`), then
`.send().await?.json::<TokenResponse>().await`.

```rust
pub struct DiscordUser {
    pub id: Id<UserMarker>,
    pub username: String,
    // avatar, discriminator, etc. not needed
}

pub async fn fetch_current_user(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<DiscordUser, reqwest::Error>
```
`GET https://discord.com/api/users/@me` with header `Authorization:
Bearer {access_token}`.

**Critical security constraint, restated from `00_VISION.md`: never log
`access_token` or any header/body containing it.** Double-check every
`tracing::*!` call added in this stage doesn't `{:?}`-format a
`TokenResponse` or a raw header map. The access token must exist only as
a local variable inside the `/callback` handler's function scope, used
immediately, and dropped — never written to the database, never sent back
to the browser, never stored in `AppState` or any longer-lived structure.

### `verification/roles.rs` — design

```rust
pub async fn grant_verified_role(
    http: &twilight_http::Client,
    db: &SqlitePool,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
) -> Result<(), SomeError>
```
Look up `guilds.verified_role_id` for `guild_id` (add a
`storage::models::get_verified_role_id` alongside the existing
`get_log_channel_id`, same shape). If `None` (guild never ran `/setup`),
return an error the caller turns into a clear "this server hasn't been
set up yet" message rather than silently doing nothing. Otherwise call
`http.add_guild_member_role(guild_id, user_id, role_id).await` (verified
signature in `01_ARCHITECTURE.md`/Stage 4 notes), then
`storage::models::record_verification(db, guild_id, user_id, "oauth").await`.

### `web/templates.rs` — design

Dark theme, one accent color, no fake stats/testimonials (per
`00_VISION.md` §16). Suggested structure:

```rust
pub fn layout(title: &str, body: &str) -> String { /* <head> + <style> + body */ }
pub fn landing_page() -> String
pub fn verify_page(guild_name: &str, oauth_url: &str, support_join_disclosure: Option<&str>) -> String
pub fn success_page() -> String
pub fn error_page(message: &str) -> String
```

`verify_page` must render, in visible body text (not a tooltip, not
collapsed, not fine print): what Blackwall is, what server is being
verified for (`guild_name`), what's being requested (`identify` — "your
Discord username and ID"), and — when `support_join_disclosure` is
`Some(...)` (Stage 6) — the exact disclosure sentence from
`00_VISION.md` §6. The "Continue with Discord" element is a plain `<a
href="{oauth_url}">` — no JavaScript needed for this page.

### `web/routes.rs` — design

Using `axum::Router`:

- `GET /` → `templates::landing_page()`.
- `GET /verify?guild_id=<u64>` → parse `guild_id` from the query string
  (axum's `Query<T>` extractor with a small `#[derive(Deserialize)]`
  struct); fetch the guild's name (either a fresh `http.guild(guild_id)`
  call, or read it back from the `guilds` table if it's worth caching —
  a fresh HTTP call is simpler and fine for MVP traffic levels); call
  `state.sessions.create(guild_id)` for the state token; build the
  authorize URL via `verification::oauth::authorize_url`; render
  `templates::verify_page`. On any failure (bad/missing `guild_id`, guild
  not found), render `templates::error_page` with a plain-English message
  — never a raw Rust error/panic reaching the browser.
- `GET /callback?code=<String>&state=<String>` → `state.sessions.take
  (&state)`; on `None`, render an error page ("this verification link
  expired or was already used — go back to Discord and click Verify
  again"). On `Some(pending)`: `oauth::exchange_code`, then
  `oauth::fetch_current_user`, then `roles::grant_verified_role`; on any
  step failing, render a clear error page (different message per failure
  mode where reasonable: "Discord didn't authorize this" vs. "this server
  hasn't finished setup yet" vs. a generic fallback) — and reiterate: no
  raw access token ever gets rendered into any response body or log line.
  On full success, redirect (HTTP 302, or just render) to `/success`.
- `GET /success` → `templates::success_page()`.

### `discord/verify_panel.rs` — new command

`/verify-panel`: no options, guild-only, gated on `MANAGE_GUILD` (same
pattern as `/setup`). Posts a **non-ephemeral** embed (this is meant to be
a persistent, publicly-visible panel — the deliberate exception to the
"admin commands reply ephemeral" default) with a
`ButtonStyle::Link` button labeled "Verify", `url:
format!("{public_base_url}/verify?guild_id={guild_id}")`, built via
`twilight_util::builder::message::button::ButtonBuilder::new
(ButtonStyle::Link).label("Verify").url(url).build()` wrapped in
`ActionRowBuilder::new().component(button).build()`, sent via
`http.create_message(channel_id).embeds(&[embed]).components(&[Component::
ActionRow(action_row)]).await`. A `ButtonStyle::Link` button opens the URL
directly in the user's browser — **it does not fire an
`InteractionCreate` event**, so no button-click handler is needed; the
entire "click Verify" interaction is just a normal link click. Verify
`ButtonBuilder`/`ActionRowBuilder`'s exact API against
`twilight-util-<version>/src/builder/message/{button,action_row}.rs` if
the installed version has changed by the time this is implemented (it was
confirmed present and matching this description in
`twilight-util-0.17.0` during this session).

### `main.rs` changes

- Build one shared `reqwest::Client` (consider a short overall timeout,
  e.g. `.timeout(Duration::from_secs(10))`, since these are
  user-facing/interactive requests, not background jobs) and store it in
  `AppState`.
- Build `verification::sessions::SessionStore::default()` and store it in
  `AppState`.
- After building `AppState`, check `config.discord_client_secret`: if
  `Some`, `tokio::spawn(web::run(Arc::clone(&state), config.web_bind_addr))`
  alongside the existing `gateway::run(...).await`; if `None`, log the
  warning described above and proceed exactly as before.

### Testing plan for this stage

The full OAuth round-trip (real Discord login + consent screen) requires
a human clicking through it in a real browser with a real Discord
session — this cannot be verified by an AI agent alone. What *can* and
should be verified programmatically/by inspection before calling this
stage done:
1. `cargo build` / `clippy` / `fmt` clean, as always.
2. The smoke test (bot still starts, gateway still connects) — confirms
   the web server spawning doesn't break the existing bot even when
   `DISCORD_CLIENT_SECRET` is unset.
3. With `DISCORD_CLIENT_SECRET` set, start the bot and independently
   fetch `GET http://localhost:8080/verify?guild_id=<real test guild id>`
   (via `curl`, or a browser-preview tool if available) — confirm the
   page renders, and **manually inspect the "Continue with Discord" link's
   `href`** to confirm it's a well-formed
   `https://discord.com/api/oauth2/authorize?...` URL with the right
   `client_id`, `redirect_uri`, `scope=identify`, and a `state` value that
   looks like the generated token.
4. Hand off to the human for the actual click-through test: click Verify
   in Discord → confirm the Discord consent screen shows the correct app
   name and only the `identify` scope → approve → confirm redirect back
   to `/success` → confirm the verified role was actually applied in
   Discord → confirm a row appeared in the `verified_users` table.

---

## Stage 6 — Support server join disclosure + legal pages

Builds directly on Stage 5's OAuth plumbing.

### New environment variable

```
SUPPORT_GUILD_ID   optional. The bot's own community/support server's ID.
                   If unset, the support-join feature is unavailable
                   bot-wide regardless of any per-guild
                   support_join_enabled setting.
```

### Behavior

`guild_settings.support_join_enabled` already exists (defaulting to `0`/
off) as of Stage 4 but nothing reads it yet. In this stage:
- `web/routes.rs`'s `GET /verify` handler checks
  `storage::models::get_guild_settings(...).support_join_enabled` (this
  field needs adding to the `GuildSettings` struct/query, mirroring
  `anti_spam_enabled`/`anti_scam_enabled`) **and** whether
  `SUPPORT_GUILD_ID` is configured at all. Only if both are true does
  `oauth::authorize_url` get called with `include_guilds_join: true`
  (requesting `identify guilds.join` instead of just `identify`), and
  only then does `verify_page` render the disclosure sentence (verbatim,
  from `00_VISION.md` §6): *"By continuing, you authorise this app to
  verify your Discord account. This may also add you to our official
  support/community server so you can receive support, updates, and
  security alerts."* This sentence must be regular visible body text —
  not a tooltip, not inside a `<details>`/collapsed element, not
  small/low-contrast text.
- In `GET /callback`, after `roles::grant_verified_role` succeeds
  (primary verification for the *original* guild always happens first and
  independently): if `guilds.join` was granted, attempt to add the user to
  `SUPPORT_GUILD_ID` using the **bot's** token for auth and the user's
  OAuth **access token** in the request body — this is a distinct
  Discord endpoint (`PUT /guilds/{guild.id}/members/{user.id}`, "Add Guild
  Member"). Check whether `twilight_http::Client` exposes this (a method
  likely named `add_guild_member(guild_id, user_id)` with a builder
  method to set the `access_token` field, e.g.
  `.access_token(token)`) — **verify this against
  `twilight-http-<version>/src/request/guild/member/add_guild_member.rs`
  (or similar path) before writing this code**, following the same
  "grep the installed source" discipline used throughout this project. If
  it's already a member, Discord's endpoint returns success/no-op — treat
  that as success, not an error.
- **Support-join failure must never fail primary verification.** Structure
  this as: grant the primary verified role (hard requirement, propagate
  errors to the user) → *then*, only if that succeeded and
  `guilds.join` was requested, attempt the support-server add as a
  best-effort step whose failure only logs a `tracing::warn!` and still
  redirects to `/success` normally. The success page's copy can
  optionally mention "you've also been added to our support server" only
  when that step actually succeeded — don't claim it happened if it
  didn't.
- Log embed: per `00_VISION.md` §10, "OAuth support-server join
  success/fail" should get its own log embed (sent to the *original*
  guild's log channel, since that's the server whose owner cares that
  their member did or didn't get added to the support server).

### Legal pages

`web/routes.rs` gains:
- `GET /privacy` — plain-English privacy policy. Must accurately state:
  what's collected (Discord user ID, guild ID, verification timestamp,
  verification method — nothing else, matching `verified_users`'
  actual columns), why (to grant/track the verified role), retention (no
  automatic deletion process exists yet — say so honestly rather than
  implying one does; flag this as a real future to-do, not something to
  gloss over), no third-party sharing, and how to request removal (even
  if that's currently "contact the server owner / bot operator" rather
  than a self-service flow — don't invent a mechanism that doesn't
  exist).
- `GET /terms` — plain-English terms: acceptable use, no warranty, the
  service can change or be withdrawn, not a substitute for Discord's own
  ToS.
- `GET /support` — explains what the support server is, why someone might
  land there, and provides a manual join link/invite as a fallback for
  anyone who didn't grant `guilds.join` (or for whom the automatic add
  failed) — the auto-join is a convenience, never the *only* way to reach
  support.

`templates::landing_page()`'s footer should link to all three, plus
(already existing) the add-bot/support-server buttons from
`00_VISION.md` §12.

**Stage 6 completion checklist**, restated from `00_VISION.md` §6, to
verify literally line-by-line before considering this done: state
parameter ✅ (Stage 5), CSRF ✅ (Stage 5), short-lived sessions ✅ (Stage
5), tokens never exposed to frontend ✅ (verify again for this stage's new
code path), tokens never logged ✅ (verify again), minimal data stored ✅
(`verified_users` unchanged), privacy policy exists ✅ (this stage),
disclosure is visible not hidden ✅ (this stage, verify by actually
reading the rendered HTML, not just the Rust source), joining the support
server is never secret/required/a dark pattern ✅ (this stage).

---

## Stage 7 — Anti-raid join monitoring + lockdown

### Gateway intent change (required, portal + code)

Add `Intents::GUILD_MEMBERS` to `gateway.rs`'s intent list — this is a
**privileged intent**, same category as `MESSAGE_CONTENT` (Stage 1): it
must be toggled on in the Developer Portal (Bot → Privileged Gateway
Intents → "Server Members Intent") *and* requested in code, or the
gateway connection is refused. Needed to receive `MemberAdd`/`MemberRemove`
events at all.

### `moderation/raid.rs` — design

Same shape as `SpamTracker`:

```rust
struct JoinEvent {
    at: Instant,
    user_id: Id<UserMarker>,
    account_created_at: Timestamp, // decoded from the snowflake, see below
    has_avatar: bool,
}

#[derive(Default)]
pub struct JoinTracker {
    activity: DashMap<Id<GuildMarker>, VecDeque<JoinEvent>>,
}
```

On `Event::MemberAdd`, record + prune to a rolling window (suggest 60
seconds — longer than spam's 10s window, since raids unfold over a
somewhat longer timescale than a single burst of messages), then check:
- **Join burst:** N+ joins within the window (suggest starting at 10 —
  tune based on real server sizes; a small server's "normal" join rate is
  very different from a large one's, which is exactly the kind of thing
  `/config` should eventually make adjustable per-guild rather than a
  single global constant).
- **Suspicious composition:** a high proportion of the recent joins are
  either avatar-less or very-new accounts (see below) — e.g. if 5+ of the
  last 10 joins are both no-avatar *and* under some age threshold.

### Decoding account creation time from a Discord snowflake

Needed for "is this a suspiciously new account?" This is exactly the
first genuine use case for `utils/ids.rs` from the target architecture
(`00_VISION.md` §14) — create it now rather than inlining the math into
`raid.rs`, since "decode a snowflake's timestamp" is a generic utility,
not raid-specific domain logic. Discord's snowflake format: the top 42
bits (after shifting right by 22) are milliseconds since the **Discord
epoch**, `2015-01-01T00:00:00.000Z` (i.e. `1420070400000` ms since the
Unix epoch). So: `unix_millis = (snowflake >> 22) + 1420070400000`. Verify
`twilight_model::util::Timestamp` has (or needs) a `from_micros`/
`from_secs`-equivalent constructor accepting milliseconds, or convert to
seconds first — `Timestamp::from_secs` was already confirmed to exist in
Stage 3's work (used for `spam::timeout_until`); reuse that.

```rust
// utils/ids.rs
pub fn snowflake_created_at<T>(id: Id<T>) -> Timestamp {
    const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
    let unix_millis = (id.get() >> 22) + DISCORD_EPOCH_MILLIS;
    Timestamp::from_secs((unix_millis / 1000) as i64)
        .expect("snowflake-derived timestamp out of range")
}
```

"Suspicious new account" threshold: suggest 7 days for a first pass
(`Timestamp` supports comparison — check its exact API, since it may not
directly implement `Ord`/subtraction and might need converting to
`OffsetDateTime`/Unix-seconds first for the comparison).

### Response actions (on raid detected)

Per `00_VISION.md` §4: enable lockdown (see below), increase verification
level if possible (`http.update_guild(guild_id).verification_level
(VerificationLevel::High)` or similar — verify exact builder method
name/`VerificationLevel` variants against source), pause invites if
possible (Discord doesn't have a single "disable all invites" toggle;
approximate by deleting active invites via `http.guild_invites(guild_id)`
+ `http.delete_invite(code)` for each, or by relying on the
lockdown channel overwrites below to prevent new-member messaging —
decide based on what's actually achievable via the API once this stage is
reached), require verification (skip if `verification_enabled` isn't
built into an enforcement mechanism yet — this may still be aspirational
depending on what Stage 5/6 actually enforce vs. just track), timeout new
users matching the suspicious criteria, alert staff with a "raid timeline"
log embed (list each flagged join: user, account age, avatar status,
timestamp), and gate all of this on `guild_settings.anti_raid_enabled`
(same toggle-check pattern as scam/spam — **don't repeat the Stage 2/3
gap where the column existed before the check did**).

### `/lockdown` and `/unlockdown`

- `/lockdown` (gated `MANAGE_GUILD`, same pattern as `/setup`): for every
  text channel (or, if scoping down for MVP, just channels without an
  existing restrictive `@everyone` overwrite already), add/update a
  permission overwrite denying `SEND_MESSAGES` for the `@everyone` role,
  and/or raise slowmode. **Before changing anything, snapshot the current
  per-channel `@everyone` overwrite state** so `/unlockdown` can restore
  exactly what was there before — don't just delete the overwrite
  afterward, since the channel might have already had a restrictive
  overwrite pre-lockdown that deleting would incorrectly clear. Store this
  snapshot somewhere durable across a bot restart — either a dedicated
  small table (e.g. `lockdown_snapshots(guild_id, channel_id,
  everyone_overwrite_json)`) or, if Stage 9's `backups` table already
  exists by the time this is built, consider reusing its
  `backup_json`-blob-per-guild shape instead of inventing a parallel
  mechanism — evaluate build order against Stage 9 when this is actually
  implemented. Set `guilds.lockdown_enabled = 1`.
- `/unlockdown`: reverse using the snapshot (restore each channel's exact
  prior `@everyone` overwrite, including "no overwrite existed before" as
  a valid prior state to restore to), set `guilds.lockdown_enabled = 0`.
- Both the raid-triggered automatic lockdown and the manual `/lockdown`
  command should call the **same** underlying
  `actions::lockdown::engage(...)` function (this is the first genuinely
  reused-from-multiple-call-sites action, and per `01_ARCHITECTURE.md`'s
  note, this is exactly when creating the `actions/` module stops being
  premature — do it now, don't inline it twice).

Permissions needed on the bot's invite: `MANAGE_CHANNELS` (overwrites),
`MANAGE_GUILD` (verification level changes) — add to the documented
required-permissions list.

---

## Stage 8 — Anti-nuke monitoring + permission risk detection

### Gateway event for audit-log entries

Modern Discord API exposes audit-log entries as a **gateway event**
(`GUILD_AUDIT_LOG_ENTRY_CREATE`), which is the correct approach here —
**do not** build a polling-the-REST-audit-log-endpoint mechanism, that's
the older/hackier pattern this API generation was specifically designed
to replace. **First verification step when starting this stage:** grep
the installed `twilight-model` source for an `AuditLogEntryCreate` variant
on the `Event` enum (same file where `MessageCreate`/`InteractionCreate`
were confirmed:
`twilight-model-<version>/src/gateway/event/mod.rs`), and check
`twilight-gateway`'s `Intents` for whatever intent gates it (likely named
something like `GUILD_MODERATION` — again, verify against source, don't
assume the name). This determines the exact shape of everything else in
this stage, so do this check first, before writing any code.

### `moderation/nuke.rs` — design

Same tracker shape again: `DashMap<(Id<GuildMarker>, Id<UserMarker>),
VecDeque<AuditEvent>>` keyed by (guild, **actor**) this time, not
(guild, message-author). Watch for `action_type` values matching:
`CHANNEL_DELETE`, `ROLE_DELETE`, `MEMBER_BAN_ADD` (repeated = mass ban),
`MEMBER_KICK` (repeated = mass kick), `WEBHOOK_CREATE`, `ROLE_UPDATE`
(granting a dangerous permission), `MEMBER_ROLE_UPDATE` (granting an
admin-capable role to someone), `GUILD_UPDATE` (dangerous setting
changes), bot additions. Trigger threshold: e.g. 3+ of these by the same
non-owner actor within 30 seconds.

### Response actions

Per `00_VISION.md` §7: strip the actor's dangerous roles immediately
(`http.remove_guild_member_role` for each role found to grant a dangerous
permission), quarantine them (apply `guilds.quarantine_role_id`, consider
also stripping their other roles), ban/kick if the server has configured
that aggressiveness (a setting not yet designed — decide during this
stage whether this needs a new `guild_settings` column, e.g.
`nuke_response` as an enum-ish text field: `"quarantine"` vs
`"kick"` vs `"ban"`), trigger the same `actions::lockdown::engage(...)`
from Stage 7, restore deleted channels/roles from the most recent backup
if Stage 9 exists yet (if not, skip this part of the response for now —
don't block Stage 8 on Stage 9 being done first, just don't claim
restoration happened if it didn't), and **alert the owner directly via
DM** in addition to the normal log-channel embed — the log channel itself
may have just been one of the things deleted by the attacker, so it can't
be the only alert path for this specific detector.

### `moderation/permissions.rs` — the flagged retrofit

Extract the permission-risk-checking logic currently living inline in
`discord/setup.rs` (the `@everyone`-dangerous-permissions checks and the
Administrator-member-counting) into this new module, as pure functions
taking already-fetched `&[Role]`/`&[Member]`/owner ID and returning
findings — no I/O inside these functions, matching the "pure `check()`"
detector shape from `01_ARCHITECTURE.md`. Update `discord/setup.rs` to
call the extracted functions instead of duplicating the logic. Do this
**before** writing `/security-score` below, not after, so it's written
once and reused, not copy-pasted and then reconciled later.

### `/security-score` command

New command, ephemeral, gated `MANAGE_GUILD`. Runs the extracted
`moderation::permissions` checks plus additions: staff role-hierarchy risk
(is a role that looks like "staff" — heuristic: has `KICK_MEMBERS`/
`BAN_MEMBERS`/`MANAGE_MESSAGES` but not `ADMINISTRATOR` — positioned below
a role a regular member could plausibly self-assign, e.g. via a
reaction-role bot or integration); bot role too low to moderate at all
(compare the bot's own highest role `position` against the roles it would
need to act on — if the bot can't out-rank even its own quarantine role,
none of anti-nuke's remove-role/quarantine actions can work, which is
worth surfacing loudly). Scoring: start at 100, subtract fixed amounts per
finding (suggested starting points, tune later: −30 `@everyone` has
Administrator, −15 `@everyone` can create invites, −10 per member with
unexplained Administrator beyond the owner, −20 if the bot's role can't
out-rank what it needs to manage). Reply with the score, a Critical list,
a Medium list, and plain-English suggested fixes for each finding —
matching `00_VISION.md` §8's exact requested shape.

---

## Stage 9 — Backup and restore

### New database table

```sql
CREATE TABLE IF NOT EXISTS backups (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id    TEXT NOT NULL,
    backup_json TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

Deliberately one JSON blob per backup, not a fully normalized multi-table
schema — matches the original spec's flat design exactly
(`00_VISION.md` §13) and is dramatically simpler for a beginner to reason
about than a relational representation of "roles, channels, overwrites,
categories" with all their foreign keys. Define a plain
`#[derive(Serialize, Deserialize)] struct GuildBackup { roles: Vec<RoleBackup>,
channels: Vec<ChannelBackup>, guild_name: String, verification_level:
... }` (and nested `RoleBackup`/`ChannelBackup` structs capturing name,
permissions/overwrites, position, type) and `serde_json::to_string`/
`from_str` it into/out of the `backup_json` column.

### `/backup`

Gated `MANAGE_GUILD`. Fetch current roles (`http.roles`) and channels
(`http.guild_channels`, including each channel's `permission_overwrites`
field) and basic guild info (`http.guild`), build a `GuildBackup`,
serialize, `INSERT` a row. Reply with a summary: "Backed up N roles, M
channels."

### `/restore`

MVP scope: restore the **most recent** backup only (no `backup_id`
option yet — that's a reasonable, explicitly-deferred future enhancement
once there's a reason to want an older snapshot, not needed for a first
version). For each role/channel in the stored backup that no longer
exists (matched by **name**, since Discord IDs can't be recreated with
their original value), recreate it. **Explicitly and honestly report the
limitation** in the restore summary: recreated channels/roles get *new*
Discord IDs, so anything elsewhere that referenced the old IDs (other
bots' configuration, saved permission overwrites pointing at a
now-nonexistent role ID) will need manual reattachment — this is a
fundamental Discord API constraint (you cannot choose an object's
snowflake), not a bug to "fix," so the right move is transparency, not
silence.

### Not in MVP scope, noted so it isn't accidentally half-built

Scheduled/automatic backups (e.g. nightly) are a reasonable future
addition but are not required for the MVP — manual `/backup` only, for
now. An automatic pre-restore-triggered-by-nuke-response backup (Stage
8's "restore deleted channels/roles if a backup exists" behavior) only
needs *reading* the most recent backup, not creating a new one on the fly
during an active attack — don't conflate these two behaviors.

---

## Stage 10 — Owner dashboard

The website already exists (from Stage 5/6); the dashboard is an
**authenticated extension of the same `axum` server**, not a new project.

### Two distinct OAuth flows — keep them clearly separate

The member-verification flow (`/verify`, Stage 5/6, scope `identify`
[`+ guilds.join`]) and the **owner dashboard login** flow are different
purposes with different scope needs and must not be conflated in the UI
or the code:
- Dashboard login needs `identify + guilds` (the `guilds` scope, to list
  which servers the logged-in user administers via
  `GET /users/@me/guilds`, cross-referenced against which of those guilds
  the bot is actually in and where the user has `MANAGE_GUILD` or is the
  owner).
- This should be a **separate route** (e.g. `/dashboard/login`, its own
  `redirect_uri` registered in the Developer Portal alongside `/callback`)
  and ideally explained on the landing page as a distinct "Manage your
  server" action, distinct from the "Verify" flow a regular member would
  use.

### Session persistence (new problem, not yet solved by anything built
so far)

Unlike member verification (a one-shot flow — exchange, grant role,
done), the dashboard needs the user to **stay logged in** across
multiple page loads. `verification::sessions::SessionStore`'s one-time,
10-minute-expiry design is *not* reusable as-is for this — it's solving a
different problem (CSRF-protecting a single redirect round-trip, not
long-lived authentication). This needs its own mechanism, to be decided
when this stage starts: a signed session cookie (candidates: the
`tower-sessions` crate, or a hand-rolled signed-cookie value using
something already in the dependency tree) mapping to a
`DashMap<SessionId, LoggedInUser>` (or a new small DB table, if
persistence across a bot restart matters — decide based on whether "stay
logged in across a bot redeploy" is actually a requirement) with a longer
expiry (suggest 24 hours) and some form of refresh-on-activity. This is a
genuinely new architectural decision, not a mechanical extension of
existing patterns — budget real design time for it rather than assuming
it's a copy of `SessionStore`.

### New database table

```sql
CREATE TABLE IF NOT EXISTS security_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id    TEXT NOT NULL,
    user_id     TEXT,
    event_type  TEXT NOT NULL,
    severity    TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

As flagged at the top of this document: **don't wait until Stage 10 to
create this table and start writing to it.** Every moderation action
since Stage 2 (scam deletion, spam timeout) should already be recording
here by the time Stage 10 starts, or the dashboard's "recent incidents"
page launches with no historical data and a painful retrofit across every
past action. Add this table and the `record_security_event(...)` helper
as early as Stage 5 or 6, and call it from every `handle_*_violation`
function in `gateway.rs` going forward (including retroactively from the
scam/spam handlers that already exist).

### Pages

- **Server list**: guilds where the logged-in user is the owner or has
  `MANAGE_GUILD`, cross-referenced against the bot's own guild membership
  (via `GET /users/@me/guilds`'s returned permission bits, joined against
  which guild IDs exist in the local `guilds` table).
- **Security score**: call the `moderation::permissions` functions
  extracted in Stage 8 directly (not by shelling out to the Discord
  command) — this is exactly why extracting them as pure functions in
  Stage 8 mattered.
- **Recent incidents**: query `security_events` for the selected guild,
  most recent first.
- **Protection toggles**: read/write `guild_settings`. **Share one
  underlying function with whatever `/config` command ends up doing**
  (not yet designed in detail — `/config` is listed in the original
  command list in `00_VISION.md` §1 but hasn't been scoped stage-by-stage
  the way the others have; scope it when this stage or an earlier one
  needs it, and make sure the website and the Discord command call the
  same `storage::models` write function so they can never drift out of
  sync with each other).
- **Verification settings**: `verified_role_id`, `support_join_enabled`,
  etc. — same read/write-`guild_settings`-and-`guilds` pattern.
- **Logs**: a paginated view over `security_events`, essentially the same
  query as "recent incidents" with pagination added.
- **Backup status**: list `backups` rows for the guild, with a restore
  action (gate this behind a confirmation step in the UI — restoring is
  a real, visible, hard-to-fully-undo action on the user's server, and
  per the top-level "confirm before risky actions" principle this
  project's own development process has followed throughout, the
  dashboard should hold itself to the same standard it'd want from an AI
  agent operating on its behalf).

Keep the very first dashboard version simple, per `00_VISION.md` §12 —
resist the urge to build all of the above in one pass; consider splitting
Stage 10 into its own sub-stages (read-only pages first, then toggles,
then restore-with-confirmation last) rather than treating "the dashboard"
as one atomic unit of work.
