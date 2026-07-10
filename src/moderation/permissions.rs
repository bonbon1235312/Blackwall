use std::collections::HashSet;

use twilight_http::Client as HttpClient;
use twilight_model::guild::{Guild, Permissions, Role};
use twilight_model::id::{Id, marker::GuildMarker};

/// A handful of cheap, high-signal permission checks — not a full audit,
/// just the kind of thing worth surfacing immediately. Shared by `/setup`
/// (folded into one flat warnings list) and `/security-score` (shown
/// separately by severity, plus the extra checks that only make sense as
/// part of a dedicated audit — see `discord::security_score`).
///
/// Split into `critical`/`medium` rather than a single list with a
/// severity enum baked into each string — simple enough for what two
/// severity tiers actually need, and keeps callers that only want a flat
/// list (like `/setup`) from having to unwrap anything.
pub struct PermissionFindings {
    pub critical: Vec<String>,
    pub medium: Vec<String>,
}

impl PermissionFindings {
    /// All findings, critical first — what `/setup`'s single "Warnings"
    /// field wants.
    pub fn flattened(&self) -> Vec<String> {
        self.critical
            .iter()
            .chain(self.medium.iter())
            .cloned()
            .collect()
    }

    /// 100 minus a flat per-finding penalty by severity. Lives here (not
    /// duplicated in both `/security-score` and the owner dashboard) so the
    /// two can never disagree about what a server's score is.
    pub fn score(&self) -> i64 {
        const CRITICAL_PENALTY: i64 = 30;
        const MEDIUM_PENALTY: i64 = 10;

        (100 - self.critical.len() as i64 * CRITICAL_PENALTY
            - self.medium.len() as i64 * MEDIUM_PENALTY)
            .max(0)
    }
}

/// Checks `@everyone`'s permissions and counts members with effective
/// Administrator, using already-fetched guild/role data (the caller is
/// responsible for fetching `guild`/`roles` — this function does no I/O
/// of its own except the one member-list fetch needed to count admins,
/// which only happens if there's an admin-granting role to look for).
pub async fn check(
    http: &HttpClient,
    guild_id: Id<GuildMarker>,
    guild: &Guild,
    roles: &[Role],
) -> PermissionFindings {
    let mut critical = Vec::new();
    let mut medium = Vec::new();

    // The @everyone role's ID is always identical to the guild's ID.
    if let Some(everyone) = roles.iter().find(|role| role.id.get() == guild_id.get()) {
        if everyone.permissions.contains(Permissions::ADMINISTRATOR) {
            critical.push("@everyone has Administrator — this is extremely dangerous.".to_string());
        }
        if everyone.permissions.contains(Permissions::CREATE_INVITE) {
            medium.push("@everyone can create invites.".to_string());
        }
        if everyone.permissions.contains(Permissions::MENTION_EVERYONE) {
            medium.push("@everyone can mention @everyone/@here.".to_string());
        }
    }

    let admin_role_ids: HashSet<_> = roles
        .iter()
        .filter(|role| role.permissions.contains(Permissions::ADMINISTRATOR))
        .map(|role| role.id)
        .collect();

    if !admin_role_ids.is_empty() {
        // Capped at Discord's max page size — good enough to flag a risky
        // setup without paginating through every member on a huge server.
        if let Ok(response) = http.guild_members(guild_id).limit(1000).await
            && let Ok(members) = response.model().await
        {
            let admin_count = members
                .iter()
                .filter(|member| {
                    member.user.id == guild.owner_id
                        || member
                            .roles
                            .iter()
                            .any(|role_id| admin_role_ids.contains(role_id))
                })
                .count();

            if admin_count > 0 {
                medium.push(format!(
                    "{admin_count} member(s) have Administrator permission."
                ));
            }
        }
    }

    PermissionFindings { critical, medium }
}

/// Every role's ID whose permissions include Administrator — used by
/// anti-nuke to know which roles are dangerous enough to strip from an
/// actor who trips the nuke detector.
pub fn dangerous_role_ids(roles: &[Role]) -> HashSet<Id<twilight_model::id::marker::RoleMarker>> {
    roles
        .iter()
        .filter(|role| {
            role.permissions.contains(Permissions::ADMINISTRATOR)
                || role.permissions.contains(Permissions::MANAGE_GUILD)
                || role.permissions.contains(Permissions::MANAGE_ROLES)
                || role.permissions.contains(Permissions::MANAGE_CHANNELS)
                || role.permissions.contains(Permissions::MANAGE_WEBHOOKS)
        })
        .map(|role| role.id)
        .collect()
}
