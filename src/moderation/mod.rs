//! Message-content, member-join, and audit-log moderation: scam/phishing
//! and spam detection, anti-raid join monitoring, anti-nuke audit-log
//! monitoring, ban/kick-evasion timing correlation, and shared
//! permission-risk checks. Each concern gets its own file so none of them
//! turn into one giant "does everything" module.

pub mod evasion;
pub mod nuke;
pub mod permissions;
pub mod raid;
pub mod scam;
pub mod spam;
