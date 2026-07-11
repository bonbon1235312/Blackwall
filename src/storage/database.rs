use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Opens a connection pool to the shared Supabase Postgres database.
///
/// Unlike the SQLite version this replaced, this does **not** create
/// tables — the schema now has one authoritative source
/// (`blackwallsite/supabase/schema.sql`, run once by hand in Supabase's
/// SQL editor), not two copies that could drift out of sync. That schema
/// is shared with the owner-dashboard website, so re-declaring it here in
/// SQLite-flavored `CREATE TABLE` statements would be actively wrong, not
/// just redundant.
pub async fn connect(database_url: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .expect(
            "failed to connect to Postgres — check DATABASE_URL is Supabase's direct connection \
            string (Project Settings -> Database -> Connection string -> URI) and that the \
            schema from supabase/schema.sql has been run",
        );

    // Fails loudly and immediately if the schema hasn't been applied yet,
    // rather than as a confusing error the first time a real event needs
    // the `guilds` table.
    sqlx::query("SELECT 1 FROM guilds LIMIT 1")
        .fetch_optional(&pool)
        .await
        .expect(
            "connected to Postgres, but the `guilds` table is missing — run \
            supabase/schema.sql in the Supabase SQL editor first",
        );

    pool
}
