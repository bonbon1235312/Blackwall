# Blackwall project documentation

This folder is a complete, standalone record of Blackwall's design,
current state, and remaining plan — detailed enough that the project
could be reconstructed and continued correctly without access to the
conversation that produced it. Read in this order:

1. **[00_VISION.md](00_VISION.md)** — the original brief in full: what
   Blackwall is, the non-negotiable principles (beginner-friendly code,
   MVP-first, no dark patterns, secrets hygiene), the complete tech stack,
   every feature area at its target end-state, the database schema
   target, the target file tree, the intended build order, and the
   success criteria. Start here — everything else is downstream of this.

2. **[01_ARCHITECTURE.md](01_ARCHITECTURE.md)** — the system as actually
   built: the shared-state design (`AppState`), the module tree with a
   description of every file, the deliberate divergences from the target
   tree in `00_VISION.md` and why, the two error-handling postures and
   when each applies, the database as actually implemented, the
   command/interaction handling shape, the moderation-detector shape, the
   exact `Cargo.toml` dependency list, the exact env var list, and the
   testing/verification methodology used at every stage.

3. **[02_PROGRESS_LOG.md](02_PROGRESS_LOG.md)** — the stage-by-stage
   narrative of what was built, in what actual order (Stage 4 was pulled
   ahead of Stage 3 — the story of why is here), including the security
   incident that was caught and fixed, the runtime bug that only a real
   smoke-test run surfaced, and the exact in-progress/unresolved state
   Stage 5 was left in.

4. **[03_ROADMAP.md](03_ROADMAP.md)** — Stages 5 through 10, each written
   out to implementation-plan detail: exact new dependencies and why,
   exact new env vars, exact new database tables, exact new file trees,
   exact API calls (verified against installed crate source where noted),
   and the specific gotchas already known to bite each stage.

5. **[04_GOTCHAS_AND_LEARNINGS.md](04_GOTCHAS_AND_LEARNINGS.md)** —
   cross-cutting lessons that apply regardless of which stage you're
   working on: how to safely write code against a crate ecosystem newer
   than a model's training data, why `cargo build`/`clippy` alone aren't
   enough, the `rustls` crypto-provider trap (hit once for real in Stage
   4, and a second possible instance flagged — but empirically tested,
   not assumed — going into Stage 5), the `.env` empty-string trap, the
   secrets-hygiene incident, and a list of specific Discord/twilight API
   facts already verified against source.

6. **[05_STAGE_5_COMPLETION.md](05_STAGE_5_COMPLETION.md)** — the resumed
   Stage 5 completion note: the `reqwest`/`rustls` probe result, the
   verification website/OAuth modules, `/verify-panel`, new database
   tables, security-event retrofit, verification performed, and what still
   requires a human OAuth click-through.

7. **[06_STAGE_6_COMPLETION.md](06_STAGE_6_COMPLETION.md)** — the Stage 6
   completion note: the Vercel-hosting question and why the single-binary
   architecture was kept, the support-server join setting (now controlled
   from the `/setup` panel), the
   two-condition gate on offering `guilds.join`, the best-effort
   support-join attempt after verification, the new legal pages, and what
   was directly tested (including flipping the setting in a live SQLite
   database mid-run to confirm the OAuth scope actually changes).

8. **[07_STAGES_7_TO_10_COMPLETION.md](07_STAGES_7_TO_10_COMPLETION.md)**
   — one combined completion note for Stages 7 (anti-raid + lockdown), 8
   (anti-nuke + `/security-score`), 9 (`/backup`/`/restore`), and 10 (the
   owner dashboard), all built in one session. Covers the new
   `actions::lockdown` snapshot/restore module shared by three callers,
   the owner-immunity pattern extended to raid/nuke responses, the
   `PermissionFindings::score()` extraction so `/security-score` and the
   dashboard can't disagree, the dashboard's DB-only access-control
   design (no `guilds` OAuth scope needed), and exactly what has and
   hasn't been live-tested yet.

9. **[08_SUPABASE_MIGRATION.md](08_SUPABASE_MIGRATION.md)** — the move
   from a local SQLite file to a shared Supabase Postgres database (the
   same one `blackwallsite`, the Vercel-hosted marketing site + live
   owner dashboard, reads from). Covers every query's rewrite to
   Postgres dialect, the new `storage::cache::SettingsCache` (so the
   message-handling hot path never hits the network for a handful of
   booleans), the `security_scores` table both `/setup` and
   `/security-score` now write to, two genuinely-dead columns removed
   rather than carried forward, why the bot needs a *different*
   credential (a direct Postgres connection string) than the website
   does (the `service_role` key), and what's confirmed working versus
   still untested against a live bot connection.

## If you're an AI resuming this project

Read all nine documents fully before writing any code. Then:

- Check `Cargo.toml`/`Cargo.lock` and the actual `src/` tree against
  `02_PROGRESS_LOG.md`'s "current file inventory" and the completion notes
  (05–07) to see whether the working tree still matches what's described
  here, or whether further work has happened since these docs were
  written (in which case, treat these docs as a snapshot of a point in
  time, and prefer the actual code where they disagree).
- All 10 stages from `00_VISION.md`'s build order are now done — see
  `07_STAGES_7_TO_10_COMPLETION.md`'s "What's still open" for the honest
  gap list (no `/whitelist`, no `/config`, dashboard is read-only, backup
  scope is limited to roles/text-channels/categories). The next phase is
  live testing against a real Discord server, not further feature work,
  per the user's explicit instruction that produced Stages 7–10 in the
  first place.
- Keep using the verification methodology in
  `04_GOTCHAS_AND_LEARNINGS.md` #1 for every new dependency/API — this
  project's crate versions are newer than a lot of training data, and
  guessing has already produced wrong results at least twice in this
  build's own history (see #3 and the `reqwest` feature-name miss in
  `02_PROGRESS_LOG.md`).
- Keep the two error-handling postures, the per-guild-everything
  principle, the "active now vs. configured for a later stage" honesty in
  user-facing summaries, and the build-once-share-via-AppState pattern for
  every new detector — these are established conventions, not incidental
  details, and new code should match them by default.
