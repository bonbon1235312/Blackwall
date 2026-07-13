/// Fixed, lowercase Discord invite-link substrings. Only three real
/// domains ever serve invite links, so this doesn't need `scam.rs`'s
/// Aho-Corasick machinery — a plain substring check over a lowercased copy
/// of the message is the right amount of complexity here.
const INVITE_DOMAINS: [&str; 3] = ["discord.gg/", "discord.com/invite/", "discordapp.com/invite/"];

/// Whether `content` contains a Discord invite link.
pub fn is_invite_link(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    INVITE_DOMAINS.iter().any(|domain| lower.contains(domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_invite_domains() {
        assert!(is_invite_link("join us: discord.gg/abc123"));
        assert!(is_invite_link("https://discord.com/invite/abc123"));
        assert!(is_invite_link("DISCORD.GG/ABC123"));
    }

    #[test]
    fn ignores_unrelated_links() {
        assert!(!is_invite_link("check out https://example.com"));
        assert!(!is_invite_link("no links here at all"));
    }
}
