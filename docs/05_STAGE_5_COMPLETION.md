# Blackwall — Stage 5 completion note

This note records the resumed Stage 5 implementation that happened after
the original five-document handoff was written. Treat this as the current
state for Stage 5, superseding the "in progress" note in
`02_PROGRESS_LOG.md`.

## What changed

- Ran the required `reqwest`/`rustls` probe first. A temporary real HTTPS
  request to `https://discord.com` returned `200 OK`, so the existing
  `rustls::crypto::ring::default_provider().install_default()` startup
  line remains correct. No switch to `aws-lc-rs` was needed.
- Enabled the `reqwest` `form` feature explicitly because Discord's OAuth
  token exchange is `application/x-www-form-urlencoded`.
- Added optional web/OAuth config:
  - `DISCORD_CLIENT_SECRET`
  - `PUBLIC_BASE_URL`, defaulting to `http://localhost:8080`
  - `WEB_BIND_ADDR`, defaulting to `127.0.0.1:8080`
- Extended `AppState` with:
  - a shared `reqwest::Client` for OAuth calls
  - a `verification::sessions::SessionStore`
  - the optional Discord client secret
  - the public base URL
- Added the Stage 5 database table:
  - `verified_users`
- Added the early dashboard/audit retrofit called out in the roadmap:
  - `security_events`
  - `storage::models::record_security_event(...)`
  - scam deletions and spam timeouts now write compact event rows without
    storing deleted message bodies.
- Added `verification/`:
  - `sessions.rs` generates one-time 32-byte hex OAuth state tokens with
    a 10-minute expiry.
  - `oauth.rs` builds Discord authorize URLs, exchanges OAuth codes, and
    fetches the current Discord user. OAuth access tokens are not logged,
    persisted, or stored in `AppState`.
  - `roles.rs` grants the configured Verified role through
    `twilight-http` and records successful verification.
- Added `web/`:
  - `GET /`
  - `GET /verify?guild_id=...`
  - `GET /callback?code=...&state=...`
  - `GET /success`
  - plain Rust HTML templates with escaped dynamic values and no
    JavaScript.
- Added `/verify-panel`, a guild-only, Manage Server-gated slash command
  that posts a public link-button verification panel in the current
  channel, then replies ephemerally to the admin.

## Behavior notes

- If `DISCORD_CLIENT_SECRET` is unset, the web server is not spawned and
  the Discord bot continues to run normally.
- `/verify-panel` refuses to post a panel unless
  `DISCORD_CLIENT_SECRET` is configured, so users are not given a dead
  verification link.
- The Stage 5 OAuth scope is only `identify`. `guilds.join`, support
  server disclosure, privacy, terms, and support pages remain Stage 6.
- The full browser consent flow still needs a human test with a real
  Discord session and a matching redirect URI in the Developer Portal.

## Verification performed

- `cargo build`
- `cargo fmt --check`
- `cargo clippy --all-targets`
- `cargo test` (no tests exist yet, harness passes)
- Temporary `reqwest`/`rustls` HTTPS probe: `200 OK`
- No-secret smoke test:
  - slash commands registered to the test guild
  - web server disabled warning logged
  - gateway connected
  - `Blackwall is online` logged
- Web-enabled smoke test with a dummy client secret on
  `127.0.0.1:18080`:
  - `/` returned 200
  - `/callback?code=test&state=missing` returned the expired/used state
    error page
  - `/verify?guild_id=<test guild>` returned 200 and rendered a Discord
    authorize URL with `scope=identify`
  - gateway still connected and logged `Blackwall is online`
- Playwright visual check of the verify page:
  - desktop and mobile layouts rendered without overlap
  - console errors/warnings: 0 after adding an inline empty favicon
