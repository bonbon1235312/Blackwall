use twilight_model::id::{Id, marker::GuildMarker};

use crate::moderation::permissions::PermissionFindings;
use crate::storage::models::SecurityEvent;

pub fn layout(title: &str, body: &str) -> String {
    let mut page = String::new();

    page.push_str(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark">
  <title>"#,
    );
    page.push_str(&escape_html(title));
    page.push_str(
        r#"</title>
  <link rel="icon" href="data:,">
  <style>
    :root {
      color-scheme: dark;
      --bg: #080a0e;
      --panel: #10141b;
      --panel-strong: #151b24;
      --text: #f4f7fb;
      --muted: #a7b0bd;
      --line: #293241;
      --accent: #2ec46b;
      --accent-strong: #64e391;
      --danger: #ee5d5d;
      --focus: #f2d06b;
    }

    * {
      box-sizing: border-box;
    }

    body {
      margin: 0;
      min-height: 100vh;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background:
        linear-gradient(rgba(255,255,255,0.035) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255,255,255,0.035) 1px, transparent 1px),
        var(--bg);
      background-size: 44px 44px;
      color: var(--text);
      letter-spacing: 0;
    }

    a {
      color: inherit;
    }

    a:focus-visible {
      outline: 3px solid var(--focus);
      outline-offset: 4px;
    }

    .shell {
      width: min(100%, 1040px);
      min-height: 100vh;
      margin: 0 auto;
      padding: 32px 20px;
      display: grid;
      align-content: center;
      gap: 28px;
    }

    .brand {
      display: flex;
      align-items: center;
      gap: 12px;
      color: var(--muted);
      font-weight: 700;
    }

    .mark {
      width: 36px;
      height: 36px;
      border: 1px solid var(--accent);
      border-radius: 8px;
      display: grid;
      place-items: center;
      color: var(--accent-strong);
      background: #0d1611;
      font-weight: 900;
    }

    .panel {
      border: 1px solid var(--line);
      border-radius: 8px;
      background: rgba(16, 20, 27, 0.96);
      box-shadow: 0 24px 80px rgba(0, 0, 0, 0.38);
      overflow: hidden;
    }

    .hero,
    .content {
      padding: 34px;
    }

    .hero {
      display: grid;
      grid-template-columns: 1.15fr 0.85fr;
      gap: 30px;
      align-items: center;
      border-bottom: 1px solid var(--line);
      background: var(--panel-strong);
    }

    h1 {
      margin: 0 0 16px;
      font-size: 42px;
      line-height: 1.05;
    }

    h2 {
      margin: 0 0 12px;
      font-size: 24px;
      line-height: 1.2;
    }

    p {
      margin: 0;
      color: var(--muted);
      font-size: 16px;
      line-height: 1.65;
    }

    .lead {
      max-width: 680px;
      font-size: 18px;
    }

    .status {
      border-left: 3px solid var(--accent);
      padding: 18px 0 18px 18px;
      display: grid;
      gap: 8px;
    }

    .status strong {
      color: var(--text);
      font-size: 15px;
    }

    .content {
      display: grid;
      gap: 24px;
    }

    .detail-list {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 1px;
      background: var(--line);
      border: 1px solid var(--line);
      border-radius: 8px;
      overflow: hidden;
    }

    .detail-list div {
      min-height: 112px;
      padding: 18px;
      background: var(--panel);
    }

    .detail-list dt {
      margin: 0 0 8px;
      color: var(--text);
      font-weight: 700;
    }

    .detail-list dd {
      margin: 0;
      color: var(--muted);
      line-height: 1.55;
    }

    .notice {
      border: 1px solid rgba(238, 93, 93, 0.42);
      border-radius: 8px;
      padding: 16px;
      color: #ffd7d7;
      background: rgba(238, 93, 93, 0.08);
    }

    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      align-items: center;
    }

    .button {
      min-height: 46px;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      padding: 0 18px;
      border-radius: 8px;
      border: 1px solid var(--accent);
      background: var(--accent);
      color: #06120a;
      text-decoration: none;
      font-weight: 800;
    }

    .button.secondary {
      background: transparent;
      color: var(--text);
      border-color: var(--line);
    }

    .footer {
      color: #7f8997;
      font-size: 13px;
      display: grid;
      gap: 6px;
    }

    .footer p {
      margin: 0;
      color: inherit;
      font-size: inherit;
    }

    .footer a {
      text-decoration: underline;
      text-underline-offset: 2px;
    }

    @media (max-width: 760px) {
      .shell {
        padding: 20px 14px;
      }

      .hero {
        grid-template-columns: 1fr;
        padding: 24px;
      }

      .content {
        padding: 24px;
      }

      h1 {
        font-size: 32px;
      }

      .detail-list {
        grid-template-columns: 1fr;
      }

      .button {
        width: 100%;
      }
    }
  </style>
</head>
<body>
  <main class="shell">
    <div class="brand"><span class="mark">B</span><span>Blackwall</span></div>
"#,
    );
    page.push_str(body);
    page.push_str(
        r#"
    <footer class="footer">
      <p>Blackwall stores only the minimum verification record needed for server security.</p>
      <p><a href="/privacy">Privacy</a> &middot; <a href="/terms">Terms</a> &middot; <a href="/support">Support</a></p>
    </footer>
  </main>
</body>
</html>
"#,
    );

    page
}

pub fn landing_page() -> String {
    layout(
        "Blackwall",
        r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Fast Discord security, simple setup.</h1>
          <p class="lead">Blackwall protects servers with scam filtering, spam response, per-server setup, and a verification flow built into the Rust bot.</p>
          <div class="actions">
            <a class="button secondary" href="/support">Support Server</a>
          </div>
        </div>
        <div class="status">
          <strong>Current focus</strong>
          <p>Verification is handled through Discord OAuth and applies the configured Verified role after setup.</p>
        </div>
      </div>
      <div class="content">
        <dl class="detail-list">
          <div>
            <dt>Per server</dt>
            <dd>Log channels, roles, and settings are stored separately for every guild.</dd>
          </div>
          <div>
            <dt>Low retention</dt>
            <dd>OAuth access tokens are used for the callback request only and are not stored.</dd>
          </div>
          <div>
            <dt>Clear consent</dt>
            <dd>The verify page shows what Discord account data is requested before continuing.</dd>
          </div>
        </dl>
      </div>
    </section>"#,
    )
}

pub fn verify_page(
    guild_name: &str,
    verify_only_url: &str,
    verify_and_join_url: Option<&str>,
) -> String {
    // Whether to join Blackwall's official support server is the
    // verifying *user's* choice, made right here with two distinct
    // buttons — never a server admin's decision applied to everyone who
    // verifies. `verify_and_join_url` is `None` on Blackwall instances
    // that don't have a support server configured at all, in which case
    // there's nothing to choose between and only the plain button shows.
    let actions = match verify_and_join_url {
        Some(join_url) => format!(
            r#"<div class="actions">
          <a class="button secondary" href="{verify_only_url}">Verify only</a>
          <a class="button" href="{join_url}">Verify + join support server</a>
        </div>
        <p class="notice">
          "Verify + join support server" additionally asks Discord's permission to add you to
          Blackwall's official support/community server, through the extra <code>guilds.join</code>
          OAuth scope, so you can get help, updates, and security alerts there. Either choice grants
          this server's Verified role the same way.
        </p>"#,
            verify_only_url = escape_html(verify_only_url),
            join_url = escape_html(join_url),
        ),
        None => format!(
            r#"<div class="actions">
          <a class="button" href="{verify_only_url}">Continue with Discord</a>
        </div>"#,
            verify_only_url = escape_html(verify_only_url),
        ),
    };

    layout(
        "Verify with Blackwall",
        &format!(
            r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Verify for {guild_name}</h1>
          <p class="lead">Blackwall will ask Discord to confirm your username and user ID, then the bot will grant this server's configured Verified role.</p>
        </div>
        <div class="status">
          <strong>Requested from Discord</strong>
          <p>Your Discord username and user ID, always through the <code>identify</code> OAuth scope. Choosing to also join the support server below additionally asks for <code>guilds.join</code>.</p>
        </div>
      </div>
      <div class="content">
        <dl class="detail-list">
          <div>
            <dt>Token handling</dt>
            <dd>Your OAuth access token is used only during this callback and is not stored.</dd>
          </div>
          <div>
            <dt>Server action</dt>
            <dd>The bot grants the Verified role configured by this server's admins.</dd>
          </div>
          <div>
            <dt>Stored record</dt>
            <dd>Blackwall stores the server ID, your user ID, verification time, and method.</dd>
          </div>
        </dl>
        {actions}
      </div>
    </section>"#,
            guild_name = escape_html(guild_name),
        ),
    )
}

pub fn success_page(support_joined: Option<bool>) -> String {
    let support_note = match support_joined {
        Some(true) => {
            r#"<p class="lead">You've also been added to Blackwall's official support server.</p>"#
        }
        Some(false) => {
            r#"<p class="lead">Blackwall couldn't add you to the support server automatically. You can still join manually from the <a href="/support">support page</a>.</p>"#
        }
        None => "",
    };

    layout(
        "Verification complete",
        &format!(
            r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>You are verified.</h1>
          <p class="lead">Blackwall has confirmed your Discord account and asked the bot to grant the configured Verified role.</p>
          {support_note}
        </div>
        <div class="status">
          <strong>Done</strong>
          <p>You can return to Discord now.</p>
        </div>
      </div>
    </section>"#
        ),
    )
}

pub fn privacy_page() -> String {
    layout(
        "Privacy policy",
        r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Privacy policy</h1>
          <p class="lead">Blackwall collects the minimum needed to verify accounts and act on security events — nothing else.</p>
        </div>
        <div class="status">
          <strong>What's stored</strong>
          <p>Discord server ID, Discord user ID, verification timestamp, verification method, and compact security-event records.</p>
        </div>
      </div>
      <div class="content">
        <dl class="detail-list">
          <div>
            <dt>What is collected</dt>
            <dd>When you verify: your Discord user ID, the server ID, the time, and the method ("oauth"). When a moderation action happens: the server ID, your user ID if relevant, a short event type and description — never full message content.</dd>
          </div>
          <div>
            <dt>OAuth tokens</dt>
            <dd>Your Discord access token is used only for the single verification request that needs it and is never written to disk.</dd>
          </div>
          <div>
            <dt>No third parties</dt>
            <dd>Data is not sold, shared, or used for advertising. It's used only to operate Blackwall's security features.</dd>
          </div>
          <div>
            <dt>Retention</dt>
            <dd>Records are kept for as long as Blackwall is active in a server. There is no automatic deletion process yet — if you want a record removed, ask that server's admin or the bot operator directly.</dd>
          </div>
          <div>
            <dt>Support server</dt>
            <dd>If a server has enabled it and you approve the extra scope, your account may be added to Blackwall's official support server. This is disclosed on the verify page before you continue — never hidden.</dd>
          </div>
          <div>
            <dt>Contact</dt>
            <dd>Questions about your data should go to the admin of the server you verified with, or the operator running this Blackwall instance.</dd>
          </div>
        </dl>
      </div>
    </section>"#,
    )
}

pub fn terms_page() -> String {
    layout(
        "Terms",
        r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Terms</h1>
          <p class="lead">Plain-English terms for using Blackwall's verification website.</p>
        </div>
        <div class="status">
          <strong>Short version</strong>
          <p>Use it as intended, don't abuse it, and understand it's provided as-is.</p>
        </div>
      </div>
      <div class="content">
        <dl class="detail-list">
          <div>
            <dt>Acceptable use</dt>
            <dd>This site exists to verify Discord accounts for servers running Blackwall. Don't use it to attempt to access accounts that aren't yours or to disrupt the service.</dd>
          </div>
          <div>
            <dt>No warranty</dt>
            <dd>Blackwall is provided as-is, without guarantees of uptime or fitness for a particular purpose.</dd>
          </div>
          <div>
            <dt>Changes</dt>
            <dd>Features, scopes requested, and this site's behavior can change as Blackwall is developed further.</dd>
          </div>
          <div>
            <dt>Discord's own terms still apply</dt>
            <dd>This page doesn't replace Discord's Terms of Service or Community Guidelines — those still govern your Discord account and any server you're in.</dd>
          </div>
        </dl>
      </div>
    </section>"#,
    )
}

pub fn support_page(invite_url: Option<&str>) -> String {
    let join_action = match invite_url {
        Some(url) => format!(
            r#"<div class="actions"><a class="button" href="{}">Join the support server</a></div>"#,
            escape_html(url)
        ),
        None => r#"<p class="lead">A public invite link hasn't been configured for this Blackwall instance yet. Ask a server admin how to reach support.</p>"#.to_string(),
    };

    layout(
        "Support",
        &format!(
            r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Blackwall support</h1>
          <p class="lead">If a server you verified with has support-server join enabled, Blackwall may have already added you here automatically. You can also join manually any time.</p>
        </div>
        <div class="status">
          <strong>Why you might see this</strong>
          <p>Some servers opt their verified members into Blackwall's official support server for updates and security alerts. This is always disclosed on the verify page first.</p>
        </div>
      </div>
      <div class="content">
        {join_action}
      </div>
    </section>"#
        ),
    )
}

pub fn dashboard_list_page(guilds: &[(Id<GuildMarker>, String)]) -> String {
    let rows = if guilds.is_empty() {
        r#"<p class="lead">No servers found where you're recorded as the owner. Run <code>/setup</code> in a server you own, then come back and refresh this page.</p>"#.to_string()
    } else {
        guilds
            .iter()
            .map(|(guild_id, name)| {
                format!(
                    r#"<div><dt>{name}</dt><dd><a class="button secondary" href="/dashboard/{guild_id}">Open dashboard</a></dd></div>"#,
                    name = escape_html(name),
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    layout(
        "Your servers",
        &format!(
            r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Your servers</h1>
          <p class="lead">Servers where Blackwall has you recorded as the owner from <code>/setup</code>.</p>
        </div>
      </div>
      <div class="content">
        <dl class="detail-list">
          {rows}
        </dl>
      </div>
    </section>"#
        ),
    )
}

pub fn dashboard_guild_page(
    guild_name: &str,
    findings: &PermissionFindings,
    events: &[SecurityEvent],
) -> String {
    let score = findings.score();

    let findings_html = if findings.critical.is_empty() && findings.medium.is_empty() {
        "<p>No permission risks found.</p>".to_string()
    } else {
        let items = findings
            .flattened()
            .iter()
            .map(|finding| format!("<li>{}</li>", escape_html(finding)))
            .collect::<Vec<_>>()
            .join("");

        format!("<ul>{items}</ul>")
    };

    let events_html = if events.is_empty() {
        "<p>No recent security events.</p>".to_string()
    } else {
        let items = events
            .iter()
            .map(|event| {
                format!(
                    "<li><strong>{}</strong> &middot; {} &middot; {} &middot; {}</li>",
                    escape_html(&event.event_type),
                    escape_html(&event.severity),
                    escape_html(&event.description),
                    escape_html(&event.created_at),
                )
            })
            .collect::<Vec<_>>()
            .join("");

        format!("<ul>{items}</ul>")
    };

    layout(
        "Server dashboard",
        &format!(
            r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>{guild_name}</h1>
          <p class="lead">Security score: {score}/100</p>
        </div>
        <div class="status">
          <strong>Permission risks</strong>
          {findings_html}
        </div>
      </div>
      <div class="content">
        <h2>Recent security events</h2>
        {events_html}
        <div class="actions">
          <a class="button secondary" href="/dashboard">Back to your servers</a>
        </div>
      </div>
    </section>"#,
            guild_name = escape_html(guild_name),
        ),
    )
}

pub fn error_page(message: &str) -> String {
    layout(
        "Verification issue",
        &format!(
            r#"<section class="panel">
      <div class="hero">
        <div>
          <h1>Verification could not continue.</h1>
          <p class="lead">{}</p>
        </div>
        <div class="status">
          <strong>Next step</strong>
          <p>Go back to Discord and use the Verify button again. If this keeps happening, ask a server admin to rerun setup.</p>
        </div>
      </div>
    </section>"#,
            escape_html(message)
        ),
    )
}

fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());

    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}
