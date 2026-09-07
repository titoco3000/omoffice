//! The `connections` table: one row per open socket, room or presence.
//! Read by `chat` (its own room fan-out) and by [`super::Messenger`]'s
//! deployed delivery; see `schema::CONNECTIONS`.

use crate::core::db::{retry_on_conflict, Db, DbError};

/// One row: which user, on which open connection, in which org's room --
/// `org_id` is `None` for a presence-only connection (see the module doc).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Connection {
    pub connection_id: String,
    pub user_email: String,
    pub org_id: Option<String>,
}

/// Record a newly accepted connection, room or presence (`org_id: None`).
/// Idempotent on `connection_id`, so a repeat `$connect` is a no-op.
pub async fn record(
    db: &Db,
    connection_id: &str,
    user_email: &str,
    org_id: Option<&str>,
) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO connections (connection_id, user_email, org_id) VALUES ($1, $2, $3)
             ON CONFLICT (connection_id) DO NOTHING",
        )
        .bind(connection_id)
        .bind(user_email)
        .bind(org_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await
}

/// Forget a connection: `$disconnect`, or a lazy cleanup after `Gone`.
/// Removing an already-gone row is not an error.
pub async fn remove(db: &Db, connection_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM connections WHERE connection_id = $1")
            .bind(connection_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Look a connection up by the id API Gateway invoked the lambda with — how
/// a message event learns who sent it and which room it's in, if any.
pub async fn by_connection_id(db: &Db, connection_id: &str) -> Result<Option<Connection>, DbError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, Connection>(
        "SELECT connection_id, user_email, org_id FROM connections WHERE connection_id = $1",
    )
    .bind(connection_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Every open connection in `org_id` -- the roster a message fans out to.
pub async fn in_org(db: &Db, org_id: &str) -> Result<Vec<Connection>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, Connection>(
        "SELECT connection_id, user_email, org_id FROM connections WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every open connection `email` has, across every room or none — what
/// invite delivery reads to push a toast to all of them.
pub async fn in_user(db: &Db, email: &str) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT connection_id FROM connections WHERE user_email = $1")
            .bind(email)
            .fetch_all(pool)
            .await?;
    Ok(ids)
}
