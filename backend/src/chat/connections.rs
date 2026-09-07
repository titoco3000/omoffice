//! The `connections` table: one row per open browser socket to the chat
//! WebSocket API -- a room connection *and* a room-less presence one
//! (`chat::ws_presence` / a `$connect` with no `org_id`) share the same
//! table and the same row shape, `org_id` just nullable. `chat::lambda`'s
//! `$connect` writes it, `$disconnect` deletes it, and a room message fans
//! out by reading every row in the sender's `org_id`. See `schema::CONNECTIONS`.
//!
//! One table rather than two: a single browser socket has to serve both
//! jobs for the same tab (join an org's room, *and* still be reachable for
//! an invite's live toast even when that room isn't the invite's org, or
//! there is no room at all yet) -- so there is exactly one connection per
//! tab to look up, not a room one and a presence one to keep in sync.
//!
//! The SQL lives here rather than in `core::db` on purpose, same split
//! `users.rs`/`meetings.rs` keep with `core::db`: `core::db` owns the
//! connection, this module owns the table.

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
/// Idempotent on `connection_id`: API Gateway hands out a fresh one per
/// socket, so a conflict here would mean the same id came back from
/// `$connect` twice, which just means the second write is a no-op rather
/// than an error.
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
/// -- how a message event learns who sent it and which room it's in (if
/// any -- `org_id` is `None` for a presence connection, which
/// `chat::handle_message` treats as nothing to fan out), since the event
/// itself carries neither.
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

/// Every open connection `email` has right now, in *any* room or none --
/// unlike [`in_org`], not scoped to a single org. What `chat::push_to_user`
/// (the deployed half of an invite's live-toast delivery, see
/// `messenger::Messenger::push_toast`) reads: someone might have a chat room
/// open, a presence-only connection, or several tabs of either, and all of
/// them should get the toast.
pub async fn in_user(db: &Db, email: &str) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT connection_id FROM connections WHERE user_email = $1")
            .bind(email)
            .fetch_all(pool)
            .await?;
    Ok(ids)
}
