use twilight_model::channel::Message;

use crate::discord::source::CommandSource;
use crate::discord::{backup, config, lockdown, moderate, security_score, setup, verify_panel};
use crate::state::AppState;

/// `!` and `?` both work, simultaneously, as equivalent prefixes for every
/// command — same commands as the slash-command set, just triggered by a
/// plain message instead of Discord's own interaction UI.
const PREFIXES: [char; 2] = ['!', '?'];

/// If `message.content` starts with a recognized prefix followed by a
/// recognized command name, runs that command and returns `true`.
/// Returns `false` (does nothing) for anything else — a message that just
/// happens to start with `!`/`?` but isn't a real command falls through
/// untouched, same as it always has.
pub async fn try_handle(message: &Message, state: &AppState) -> bool {
    let Some(rest) = PREFIXES
        .iter()
        .find_map(|prefix| message.content.strip_prefix(*prefix))
    else {
        return false;
    };

    let rest = rest.trim_start();
    let (command_name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let command_name = command_name.to_ascii_lowercase();
    let args = args.trim();
    let prefix_args = (!args.is_empty()).then_some(args);

    let source = CommandSource::Message(message);

    match command_name.as_str() {
        "ping" => {
            source.reply(state, "Pong! Blackwall is online.").await;
        }
        "setup" => setup::handle_command(&source, state).await,
        "verify-panel" => verify_panel::handle(&source, state).await,
        "lockdown" => lockdown::handle_lockdown(&source, state).await,
        "unlockdown" => lockdown::handle_unlockdown(&source, state).await,
        "security-score" => security_score::handle(&source, state).await,
        "backup" => backup::handle_backup(&source, state).await,
        "restore" => backup::handle_restore(&source, state).await,
        "config" => config::handle(&source, state, prefix_args).await,
        "ban" => moderate::handle_ban(&source, state, prefix_args).await,
        "kick" => moderate::handle_kick(&source, state, prefix_args).await,
        "timeout" => moderate::handle_timeout(&source, state, prefix_args).await,
        "warn" => moderate::handle_warn(&source, state, prefix_args).await,
        // Not a recognized command — could be an unrelated message that
        // just happens to start with `!`/`?` (a common prefix for other
        // bots, or a bare exclamation/question). Do nothing, silently.
        _ => return false,
    }

    true
}
