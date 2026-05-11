use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

/// Creates and returns a Postgres connection pool.
/// Called once at startup; the pool is cloned into Axum's shared state.
pub async fn create_pool(database_url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300))
        .connect(database_url)
        .await?;

    // Verify the connection is alive
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await?;

    tracing::info!("Database pool established (max_conns={})", max_connections);
    Ok(pool)
}