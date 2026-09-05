//! The `connections` table: one row per open browser socket to the chat
//! WebSocket API. `chat::lambda`'s `$connect` writes it, `$disconnect`
//! deletes it, and a message fans out by reading every row in the sender's
//! `org_id`. See `schema::CONNECTIONS`.
//!
//! The SQL lives here rather than in `core::db` on purpose, same split
//! `users.rs`/`meetings.rs` keep with `core::db`: `core::db` owns the
//! connection, this module owns the table.

use crate::core::db::{retry_on_conflict, Db, DbError};

/// One row: which user, in which org's room, on which open connection.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Connection {
    pub connection_id: String,
    pub user_email: String,
    pub org_id: String,
}

/// Record a newly accepted connection. Idempotent on `connection_id`: API
/// Gateway hands out a fresh one per socket, so a conflict here would mean
/// the same id came back from `$connect` twice, which just means the second
/// write is a no-op rather than an error.
pub async fn record(
    db: &Db,
    connection_id: &str,
    user_email: &str,
    org_id: &str,
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

/// Forget a connection: `$disconnect`, or a lazy cleanup after
/// [`crate::core::apigw_management::ApiGatewayManagementError::Gone`].
/// Removing a row that is already gone is not an error -- there is nothing
/// left to disagree with.
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

/// Look a connection up by the id API Gateway just invoked the lambda with
/// -- how a message event learns who sent it and which room it's in, since
/// the event itself carries neither.
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
