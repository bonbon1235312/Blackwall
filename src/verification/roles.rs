use sqlx::PgPool;
use twilight_http::Client as HttpClient;
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::storage::models;

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
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
) -> Result<(), GrantVerifiedRoleError> {
    let Some(role_id) = models::get_verified_role_id(db, guild_id).await? else {
        return Err(GrantVerifiedRoleError::GuildNotSetUp);
    };

    http.add_guild_member_role(guild_id, user_id, role_id)
        .await?;

    models::record_verification(db, guild_id, user_id, "oauth").await?;

    Ok(())
}
