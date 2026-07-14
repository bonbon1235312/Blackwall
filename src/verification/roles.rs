use sqlx::PgPool;
use tokio::sync::mpsc;
use twilight_http::Client as HttpClient;
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::storage::models;
use crate::verification::events::VerificationEvent;

#[derive(Debug)]
pub enum GrantVerifiedRoleError {
    GuildNotSetUp,
    Discord(twilight_http::Error),
    Storage(sqlx::Error),
}

impl From<twilight_http::Error> for GrantVerifiedRoleError {
    fn from(source: twilight_http::Error) -> Self {
        Self::Discord(source)
    }
}

impl From<sqlx::Error> for GrantVerifiedRoleError {
    fn from(source: sqlx::Error) -> Self {
        Self::Storage(source)
    }
}

pub async fn grant_verified_role(
    http: &HttpClient,
    db: &PgPool,
    events: &mpsc::UnboundedSender<VerificationEvent>,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
) -> Result<(), GrantVerifiedRoleError> {
    let Some(role_id) = models::get_verified_role_id(db, guild_id).await? else {
        return Err(GrantVerifiedRoleError::GuildNotSetUp);
    };

    http.add_guild_member_role(guild_id, user_id, role_id)
        .await?;

    // The Discord-side security boundary this whole flow exists to
    // enforce is already satisfied at this point — recording the row is
    // bookkeeping, deferred to the batched writer instead of costing this
    // request another Postgres round-trip. A dropped receiver (writer task
    // gone) only means the process is shutting down; nothing to recover.
    let _ = events.send(VerificationEvent::Completed {
        guild_id,
        user_id,
        method: "oauth",
    });

    Ok(())
}
