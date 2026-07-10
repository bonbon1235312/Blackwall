# Blackwall — Gotchas and Learnings

Cross-cutting lessons discovered while building Stages 1–4 (and starting
Stage 5). These aren't tied to one stage — read this file in full before
touching the codebase, regardless of which stage you're picking up.

## 1. This project targets a crate ecosystem newer than most training data — verify, don't guess

The toolchain in use is `rustc 1.96.1` (installed mid-2026 via `winget
install --id Rustlang.Rustup -e`), and the resolved dependency versions
are correspondingly recent: `tokio 1.52.3`, `twilight-gateway/http/model
0.17.1`, `twilight-util 0.17.0`, `sqlx 0.9.0`, `axum 0.8.9`, `reqwest
0.13.4`, `rand 0.10.2`, `rustls 0.23.41`. An AI's training data very
likely predates some or all of these exact versions, which means
API shapes recalled "from memory" (method names, builder patterns,
feature-flag names, even which module re-exports what) can be
confidently wrong in ways that only show up as a compile error — or
worse, don't show up until runtime.

**The methodology used throughout this build, and the one to keep
using:**

1. Add the dependency with `cargo add <crate> [--features ...]` rather
   than hand-editing `Cargo.toml`. The command's own output shows the
   *actual* resolved version and the full list of available features
   (with `+`/`-` prefixes for enabled/disabled) — this is often the
   fastest way to discover a feature was renamed (see the `reqwest`
   `rustls-tls` → `rustls` example in Gotcha #3 below) without any
   guessing.
2. Run `cargo build` or `cargo fetch` to actually download the crate's
   source into `~/.cargo/registry/src/index.crates.io-<hash>/<crate>-<version>/src/`.
3. **Grep that real source directly** before writing code against it —
   `grep -n "pub fn <name>"`, `grep -n "pub struct <Name>"`, `grep -n
   "pub enum <Name>" -A 20`, etc. This was done dozens of times across
   this session (confirming, among many others: `Shard::with_config`,
   `StreamExt::next_event`'s exact `Option<Result<Event,
   ReceiveMessageError>>` return shape, `Response<T>::model()` vs.
   `Response<ListBody<T>>::model()` both existing and doing the "right"
   thing, `CommandBuilder`/`ChannelBuilder`/`RoleBuilder`'s exact method
   names, `Permissions`' exact constant names — e.g. `CREATE_INVITE`, not
   `CREATE_INSTANT_INVITE` as the human brief's own wording might suggest,
   `PartialMember.permissions`, `InteractionResponseData`'s `flags` field
   existing on the struct even though the *builder* doesn't expose a
   setter for it, `Id<T>`'s `Display`/`FromStr` impls, `bool`'s `Decode`
   impl for SQLite `INTEGER` columns in `sqlx-sqlite`, and
   `ButtonStyle::Link`/`Button::url`).
4. Only after that verification, write the Rust code, then immediately
   `cargo build` it to confirm the real compiler agrees.

Do not skip step 3 "because it's probably the same as an older version" —
several of the APIs above (Discord's own permission bit names, `sqlx`'s
default feature set, `reqwest`'s TLS feature naming) had changed in ways
that would have produced either a compile error or, worse, a
silently-wrong runtime behavior if assumed rather than checked.

## 2. `cargo build`/`clippy` are necessary but not sufficient — always smoke-test a real run

`cargo build` and `cargo clippy --all-targets` were run after essentially
every change in this session and never once produced a false negative on
a genuine compile-time issue. But **the single most impactful bug found
in this entire build (the `rustls` `CryptoProvider` panic — see #3 below)
was invisible to both.** It's a runtime-only failure: the code is
perfectly well-typed and lint-clean; it just panics the instant it tries
to open its first TLS connection, because the choice of crypto backend is
a runtime `install_default()` call, not something the type system can
check for you.

**Standing rule for this project:** any change that touches startup
sequencing, adds a dependency with its own networking/TLS/crypto stack, or
modifies the gateway connection logic gets a real smoke test —
`RUST_LOG=info timeout 20 cargo run` (or the platform equivalent),
reading the actual terminal output for "connecting to the Discord
gateway...", "Blackwall is online", and "registered slash commands..."
— *in addition to* build/clippy/fmt, not instead of them. This requires
real Discord credentials in `.env`, which is exactly why this project
keeps a real `.env` (git-ignored) alongside the blank `.env.example` —
the smoke test is treated as a normal, expected part of finishing a
stage, not an optional nice-to-have that needs special setup.

## 3. `rustls` 0.23+ needs an explicit, single, unambiguous crypto backend — and this gets *harder*, not easier, as more networking crates are added

`rustls` (used transitively by `twilight-gateway`/`twilight-http` for the
gateway websocket and any HTTPS call) will not auto-select a
`CryptoProvider` unless **exactly one** of its two interchangeable crypto
backend features (`ring` or `aws-lc-rs`) is active across the *entire*
dependency graph. If zero are active, or if two different crates in the
tree each pull in a *different* one of the two, the process panics the
first time it tries to establish a TLS connection, with the message:

> Could not automatically determine the process-level CryptoProvider from
> Rustls crate features. Call CryptoProvider::install_default() before
> this point to select a provider manually...

**What actually happened in this project:** Stage 4 hit this the moment
the bot was first actually run after the database work was added (not
because the database itself uses TLS — it doesn't; SQLite is local file
I/O — but because this was simply the first time the *smoke test*, not
just `cargo build`, was run after enough of the dependency tree had grown
that the ambiguity manifested). The fix: add `rustls` as a **direct**
dependency with `default-features = false` and *only* the `ring` feature
enabled explicitly (plus `std`/`tls12`/`logging` to keep otherwise-default
non-crypto-backend behavior), and call
`rustls::crypto::ring::default_provider().install_default().expect(...)`
as the literal first line of `main()`, before `Config::load()`, before
building any client. **Critically, `rustls`'s own *default* feature set
includes `aws_lc_rs`** — a naive `cargo add rustls --features ring`
(without `--no-default-features`) re-enables the exact ambiguity it was
meant to fix, because now both `ring` (explicitly requested) and
`aws_lc_rs` (still on by default) are compiled in simultaneously. This
was caught and corrected within the same session by re-running `cargo
add` with `--no-default-features` and re-checking the feature list in the
command's own output before proceeding.

**This may resurface in Stage 5 — status: suspected, not yet confirmed.**
Adding `reqwest` (needed for the OAuth token-exchange/user-info calls,
since `twilight-http` is bot-token-only and can't make arbitrary
user-token-authenticated requests) adds a *second* crypto backend to the
dependency tree: `reqwest`'s `rustls` feature (on `reqwest 0.13.4`) pulls
in `aws-lc-rs` by default, confirmed present in `Cargo.lock` alongside the
project's existing `ring`-only direct `rustls` dependency (`grep -n
'^name = "ring"$\|^name = "aws-lc-rs"$\|^name = "aws-lc-sys"$'
Cargo.lock` shows all three). **However — this was empirically tested
before writing this document, and as of `axum`+`reqwest`(with
`aws-lc-rs`)+`rand` being added but *not yet used anywhere in the code*,
both `cargo build` and the full smoke test (gateway connects,
commands register, "Blackwall is online" logs) still succeed with no
panic.** This makes sense given how `install_default()` actually works:
the ambiguity-detection/panic path only triggers when code calls
`CryptoProvider::get_default()` (indirectly, e.g. via
`rustls::ClientConfig::builder()`) *without* an explicit provider ever
having been installed, and tries to auto-select from enabled crate
features. Since this project's `main()` already calls
`rustls::crypto::ring::default_provider().install_default()` successfully
as its first action (and that call only fails if invoked a second time,
which it isn't), every consumer of the *process-wide default* — which is
what `twilight-gateway`/`twilight-http` use — gets `ring`, regardless of
`aws-lc-rs` merely being compiled into the binary unused. **The open,
unverified question is specifically what `reqwest`'s own TLS setup does
once it's actually exercised**: if `reqwest`/`hyper-rustls` also builds
its `ClientConfig` via the plain process-default path, it will simply pick
up the already-installed `ring` provider too, and there is no real conflict
at all — the whole "switch to `aws-lc-rs`" remediation below would turn
out to be unnecessary. If instead `reqwest` explicitly constructs its TLS
config with a *specific*, hard-coded provider (bypassing the process
default), the outcomes range from "works fine independently, just wastes
binary size on two backends" to, in the worst case, some form of runtime
conflict depending on exactly how that's implemented. **Recommended first
action of Stage 5, before writing any other code:** add one minimal,
throwaway call — have `main()` construct a `reqwest::Client` and issue one
real `GET` request to any HTTPS URL, immediately after the existing
`install_default()` call — and observe whether it succeeds or panics. Only
if it actually panics is the "unify on `aws-lc-rs`" remediation (detailed
next, and in `03_ROADMAP.md` Stage 5 "Step 0") actually necessary; if it
succeeds, delete the throwaway test and proceed with `ring` unchanged.
This is a concrete instance of gotcha #2 above (verify by actually
running it, don't reason your way to a conclusion) — this document
almost shipped an unverified "this will break" claim as fact, and the
five-minute empirical check changed the conclusion from "definite
problem" to "open question, here's a five-minute way to resolve it
either way." See `02_PROGRESS_LOG.md`'s Stage 5 section and
`03_ROADMAP.md`'s Stage 5 "Step 0" for the full decision tree; short
version, *if and only if* the throwaway `reqwest` request above actually
panics: standardize the whole project on `aws-lc-rs` instead of fighting
`reqwest`'s default, since `reqwest` wanting `aws-lc-rs` isn't going to
change and `ring` has no specific advantage identified for this project.

**General lesson, beyond this specific crate:** whenever adding *any* new
dependency that itself talks TLS/HTTPS (not just `reqwest` — this applies
to anything built on `hyper`+`rustls`, `tokio-tungstenite`+`rustls`, etc.),
check `Cargo.lock` afterward for whether it introduced a *second* crypto
backend crate (`grep -n '^name = "ring"$\|^name = "aws-lc-rs"$\|^name =
"aws-lc-sys"$' Cargo.lock`) before assuming the existing
`install_default()` call still covers it, and re-run the smoke test to
confirm.

## 4. `dotenvy`/`.env` semantics: `KEY=` (empty value) is not the same as `KEY` being absent

`env::var("KEY").ok()` returns `Some(String::new())`, not `None`, for a
`.env` line like `DATABASE_PATH=` (present, but with nothing after the
`=`). This matters a great deal for any *optional* config field meant to
fall back to a sensible default: `.env.example` templates necessarily
ship optional fields blank (that's the whole point of a template — the
user fills in what they need and leaves the rest), so a naive
`env::var("KEY").unwrap_or_else(|_| default)` will **never** apply its
default for anyone who copied the template as-is and didn't delete the
blank line, because `Ok("")` isn't an `Err`.

**Fix used throughout this project** — `config.rs`'s `non_empty_env`
helper:

```rust
fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}
```

Every optional config field (`TEST_GUILD_ID`, `DATABASE_PATH`, and every
future optional one — `DISCORD_CLIENT_SECRET`, `PUBLIC_BASE_URL`,
`WEB_BIND_ADDR`, `SUPPORT_GUILD_ID`, etc.) should be read through this
helper, not raw `env::var(...).ok()`. This bug was caught and fixed
*before* it shipped (while adding `DATABASE_PATH` in Stage 4) by noticing
the failure mode during code review rather than by hitting the actual
bug at runtime — worth being deliberately paranoid about this pattern
specifically, since it's easy to write the naive version and have it
look correct at a glance.

## 5. Never let a real secret land in a file that isn't git-ignored — even a "template"

Mid-Stage-2, the human pasted their real Discord bot token and a real
test-guild ID directly into `.env.example` (intending `.env`, presumably
by editing the wrong file, or the wrong file being open). `.env.example`
is **tracked** by git (only `.env` is in `.gitignore`); had a commit been
made at that point, the live token would have entered git history. It was
caught immediately, before any commit existed in the repo at all (`git
status`/`git log` were checked as the very first response), and fixed by
creating the real `.env` with the values and blanking `.env.example` back
to an empty template — no history to scrub, no token rotation strictly
required (though rotating a token that's touched an insecure location
even briefly is good hygiene regardless).

**General lesson:** whenever a message/diff/tool result contains what
looks like it could be a real secret (a bot token, a client secret, an
API key), the very next action should be checking (a) which file it's
about to land in or already landed in, and (b) whether that specific file
is covered by `.gitignore` — "it's a `.example`/template file" is not by
itself a safety guarantee; check the actual `.gitignore` rule, and check
git's tracked-file state (`git status`/`git ls-files`), not just the
filename's apparent intent.

## 6. Rust edition 2024 + `rustc` 1.96.1 supports stable let-chains

`if let Some(x) = a && let Some(y) = b { ... }` (chaining an `if let`
with a plain boolean condition, or another `if let`, via `&&`) compiles
and is actively suggested by `clippy` (via the `collapsible_if` lint) on
this toolchain. This project uses this form in a few places
(`gateway.rs`'s scam-check gating, `discord/setup.rs`'s member-fetch
check) — it's a genuine stable-Rust feature here, not a nightly-only
trick; keep using it where it reads more clearly than nested `if let`s,
and don't "fix" it back to nested `if let`s if you see it — that would be
*un*-doing a clippy-suggested improvement.

## 7. Specific Discord/twilight modeling facts worth remembering (verified this session, may drift in future crate versions — re-verify if the pinned versions ever change)

- `Response<T>` and `Response<ListBody<T>>` both expose a `.model()`
  method; for the `ListBody<T>` case it returns `Vec<T>` directly (it's
  documented as an alias for `.models()`), so `client.some_list_endpoint
  (...).await?.model().await?` works uniformly whether the endpoint
  returns one object or a list — no need to remember two different method
  names for the two cases.
- The `@everyone` role's `Id<RoleMarker>` is always numerically identical
  to the guild's own `Id<GuildMarker>`. They're different marker types
  (so `role.id == guild_id` won't even compile without a cast), but
  `role.id.get() == guild_id.get()` (comparing the raw `u64`) correctly
  identifies the `@everyone` role among a guild's role list without a
  separate "is this @everyone" flag to look for.
- Gateway event payloads that wrap a single value in a tuple struct
  inside a `Box` (e.g. `Event::MessageCreate(Box<MessageCreate>)`,
  `Event::InteractionCreate(Box<InteractionCreate>)`) can have that inner
  value moved out via `.0` (e.g. `interaction.0` yields an owned
  `Interaction`) even though `interaction` itself is a `Box<...>` — Rust
  specifically permits moving a field out of a `Box` (unlike `Rc`/`Arc`),
  so this is valid, idiomatic, and exactly the pattern `twilight`'s own
  examples use.
- Slash-command option values decode into a `CommandOptionValue` enum
  (`Channel(Id<ChannelMarker>)`, `Role(Id<RoleMarker>)`, `String(String)`,
  etc.) inside each `CommandDataOption { name, value }` — extracting a
  named, typed option is a `find_map` matching both the option's `name`
  string and the expected `CommandOptionValue` variant in the same match
  arm guard, e.g.:
  ```rust
  options.iter().find_map(|option| match &option.value {
      CommandOptionValue::Channel(id) if option.name == name => Some(*id),
      _ => None,
  })
  ```
- `Interaction.member` (a `PartialMember`) carries a `permissions:
  Option<Permissions>` field that Discord pre-computes as the invoking
  member's **effective permissions in the channel the interaction
  happened in** — this is documented specifically for interactions.
  Whether the equivalent field is populated on a regular gateway
  `MESSAGE_CREATE` event's embedded member object was **not verified**
  this session (flagged explicitly in `02_PROGRESS_LOG.md`'s Stage 3
  write-up) — don't assume either way; check source before relying on it
  for a message-triggered staff-bypass check.
- A bot needs both the relevant permission (`MANAGE_ROLES`, `MANAGE_CHANNELS`,
  `MODERATE_MEMBERS`/"Timeout Members", etc.) **and** a role positioned
  *above* whatever it's trying to create/manage/timeout in the guild's
  role list, or the corresponding `twilight_http` call fails at Discord's
  side (not a client-side check — Discord enforces this server-side, so
  the failure shows up as an HTTP error to handle, not something
  preventable purely in Rust).
- `sqlx-sqlite`'s `bool` type implements `Decode`/`Type` directly against
  SQLite's `INTEGER` columns — a column declared `INTEGER NOT NULL
  DEFAULT 1` can be read straight into a Rust `bool` via
  `row.try_get::<bool, _>("column_name")`, no manual `!= 0` conversion
  needed.

## 8. `sqlx` in this crate generation ships a lot of default features you probably don't want

A plain `cargo add sqlx --features sqlite,runtime-tokio` (accepting
defaults) pulls in `macros`, `migrate`, `json`, and `derive` — none of
which this project uses (no compile-time-checked `sqlx::query!` macros;
plain runtime `sqlx::query(...)` + manual `Row::try_get::<T, _>(...)`
calls are used throughout instead, deliberately, to avoid needing either
a live database connection at `cargo build` time or a `.sqlx` offline
query cache — both add ceremony that isn't worth it yet for a project
this size, and would be one more concept to explain to a first-time Rust
developer). **Always add with `--no-default-features --features
sqlite,runtime-tokio`** for this project, and only add back a feature
(e.g. `derive` for `#[derive(sqlx::FromRow)]`, or `macros` for compile-time
query checking) if a specific, concrete need for it shows up later —
don't pre-emptively re-enable them.

## 9. No migration framework — `CREATE TABLE IF NOT EXISTS` at startup, by design, for now

`storage::database::connect` runs plain `CREATE TABLE IF NOT EXISTS`
statements for every table on every startup. This works cleanly for
*adding* new tables (which is all that's happened so far — Stage 4 added
`guilds`/`guild_settings`; the roadmap adds `verified_users`,
`security_events`, `backups`, and possibly a lockdown-snapshot table
later) but does **not** handle *changing* an existing table's shape (e.g.
adding a column to `guilds`, changing a column's type). If/when that need
arises, the honest options are: (a) hand-write an `ALTER TABLE` statement
guarded by a check of whether the column already exists (SQLite doesn't
support `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` directly, so this means
querying `PRAGMA table_info(table_name)` first), or (b) adopt
`sqlx::migrate!` properly at that point. Don't reach for either
preemptively — the `CREATE TABLE IF NOT EXISTS` approach was a
deliberate, documented "right amount of complexity for now" choice
(stated in `database.rs`'s own doc comment), not an oversight.

## 10. Working environment specifics (useful if reproducing this exact setup)

- **OS:** Windows 11 Pro. **Shell used for all commands in this
  session:** Git Bash (POSIX-style paths like `/c/Blackwall`,
  `/c/Users/<user>/.cargo/bin`), not PowerShell or cmd.exe, though
  PowerShell was available as an alternative tool.
- **Rust install:** `winget install --id Rustlang.Rustup -e
  --accept-package-agreements --accept-source-agreements` — this installs
  `rustup-init.exe` and runs it, landing `cargo.exe`/`rustc.exe`/etc. in
  `%USERPROFILE%\.cargo\bin`. **The PATH update from this install does
  not propagate to an already-open shell session** — every command in
  this session that needed `cargo`/`rustc` was prefixed with `export
  PATH="$PATH:/c/Users/<user>/.cargo/bin";` because the Bash tool's shell
  sessions don't reliably re-read the updated user-level PATH from the
  Windows registry mid-session. A genuinely fresh terminal window opened
  *after* the rustup install should have `cargo`/`rustc` on `PATH`
  without needing this workaround.
- **Project root:** `C:\Blackwall`. Git-initialized by `cargo init`
  (which runs `git init` automatically when the target directory isn't
  already inside a git repo), but **no commits have been made** during
  any of this work — everything so far is uncommitted working-tree state.
  If reproducing this project from these docs into a fresh checkout,
  that also means there's no git history to consult for "what changed
  when" beyond what's written down here.
