use aho_corasick::AhoCorasick;

/// One phrase or domain fragment to watch for, paired with a human-readable
/// category so log messages can say *why* something was flagged instead of
/// just *that* it was.
struct Pattern {
    category: &'static str,
    phrase: &'static str,
}

/// Starter list of scam indicators. This is intentionally a plain list you
/// can keep extending — add a line, don't redesign anything.
///
/// Phrases are lowercase; matching is done case-insensitively (see
/// [`build_matcher`]), so casing here doesn't matter.
const PATTERNS: &[Pattern] = &[
    // --- Fake Discord Nitro gifts ---
    Pattern {
        category: "Fake Nitro gift",
        phrase: "discord-nitro.",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "discordnitro.",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "discord-gift.",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "discrod-nitro",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "dlscord-nitro",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "free nitro",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "nitro-gift.",
    },
    Pattern {
        category: "Fake Nitro gift",
        phrase: "nitro.gift",
    },
    // --- Discord brand impersonation domains ---
    Pattern {
        category: "Discord impersonation",
        phrase: "dlscord.com",
    },
    Pattern {
        category: "Discord impersonation",
        phrase: "discrod.com",
    },
    Pattern {
        category: "Discord impersonation",
        phrase: "discordapp.gift",
    },
    Pattern {
        category: "Discord impersonation",
        phrase: "discord-app.net",
    },
    // --- Steam scam links ---
    Pattern {
        category: "Steam scam",
        phrase: "steamcommunlty.com",
    },
    Pattern {
        category: "Steam scam",
        phrase: "steamcommunity.ru",
    },
    Pattern {
        category: "Steam scam",
        phrase: "steampowered-gift",
    },
    Pattern {
        category: "Steam scam",
        phrase: "steamcomminuty",
    },
    Pattern {
        category: "Steam scam",
        phrase: "steamcummunity",
    },
    // --- Free Robux / gift card scams ---
    Pattern {
        category: "Free Robux/gift card scam",
        phrase: "free robux",
    },
    Pattern {
        category: "Free Robux/gift card scam",
        phrase: "robux generator",
    },
    Pattern {
        category: "Free Robux/gift card scam",
        phrase: "free-robux.",
    },
    Pattern {
        category: "Free Robux/gift card scam",
        phrase: "free gift card",
    },
    Pattern {
        category: "Free Robux/gift card scam",
        phrase: "claim your gift",
    },
    // --- Token grabbers / IP loggers ---
    Pattern {
        category: "Token grabber / IP logger",
        phrase: "grabify.link",
    },
    Pattern {
        category: "Token grabber / IP logger",
        phrase: "iplogger.org",
    },
    Pattern {
        category: "Token grabber / IP logger",
        phrase: "iplogger.com",
    },
    Pattern {
        category: "Token grabber / IP logger",
        phrase: "2no.co",
    },
    Pattern {
        category: "Token grabber / IP logger",
        phrase: "yip.su",
    },
    // --- Fake giveaway wording ---
    Pattern {
        category: "Fake giveaway",
        phrase: "first 10 to join get",
    },
    Pattern {
        category: "Fake giveaway",
        phrase: "everyone who joins gets",
    },
    Pattern {
        category: "Fake giveaway",
        phrase: "dm me to claim",
    },
    Pattern {
        category: "Fake giveaway",
        phrase: "airdrop claim",
    },
];

/// Builds the scam-detection matcher once at startup.
///
/// Building an Aho-Corasick automaton does some work up front so that
/// checking a message against *every* pattern afterwards is a single fast
/// pass over the text, instead of running dozens of separate string
/// searches per message. That only pays off if we build it once and reuse
/// it — so this is called a single time in `main.rs`, and the result is
/// stored in `AppState` for every message handler to share.
pub fn build_matcher() -> AhoCorasick {
    let phrases: Vec<&str> = PATTERNS.iter().map(|pattern| pattern.phrase).collect();

    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(phrases)
        .expect("scam pattern list in moderation/scam.rs failed to compile")
}

/// A single detected match: which category it fell into, and the exact
/// phrase that triggered it — useful for the staff-facing log message.
pub struct ScamMatch {
    pub category: &'static str,
    pub phrase: &'static str,
}

/// Checks a message's content against the compiled matcher.
///
/// Returns the first match found, if any. A single hit is enough reason to
/// act on the message — we don't need to find every match to justify
/// deleting it.
pub fn check(matcher: &AhoCorasick, content: &str) -> Option<ScamMatch> {
    let found = matcher.find(content)?;
    let pattern = &PATTERNS[found.pattern().as_usize()];

    Some(ScamMatch {
        category: pattern.category,
        phrase: pattern.phrase,
    })
}
