//! The `ws_connections` table: one row per open `apigw` connection -- a
//! socket API Gateway's WebSocket API holds, not this process. Only this
//! build's `Messenger` (see [`super`]) reads or writes it, and only this
//! build even links the code to: a local build compiles `local_web_socket`
//! instead, whose sockets never touch a database at all -- see the module
//! doc on `core::communication::web_messenger` for why. A
//! `$connect`/`$default`/`$disconnect` event is its own, independent
//! Lambda invocation sharing no memory with the one before it, so "who
//! does this connection id belong to" has nowhere to live but here.

use crate::core::data::db::{retry_on_conflict, Db, DbError};

/// Record a newly accepted socket. Idempotent on `id`, so a repeat
/// `$connect` is a no-op.
pub async fn record(db: &Db, id: &str, user_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO ws_connections (id, user_id) VALUES ($1, $2)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await
}

/// Forget a socket: `$disconnect`, or a lazy cleanup after a push came back
/// `Gone`. Removing an already-gone row is not an error. Returns the user
/// it belonged to, if there was a row at all -- `super::forget_apigw` uses
/// that to fire a `DISCONNECTED` on the messenger's `CONNECTION` channel.
pub async fn remove(db: &Db, id: &str) -> Result<Option<String>, DbError> {
    let pool = db.pool().await?;
    let row = retry_on_conflict(|| async move {
        sqlx::query_as::<_, (String,)>("DELETE FROM ws_connections WHERE id = $1 RETURNING user_id")
            .bind(id)
            .fetch_optional(pool)
            .await
    })
    .await?;
    Ok(row.map(|(user_id,)| user_id))
}

/// Look a socket up by the id its own event carried -- how an incoming
/// frame learns who sent it.
pub async fn by_id(db: &Db, id: &str) -> Result<Option<String>, DbError> {
    let pool = db.pool().await?;
    let user_id: Option<String> =
        sqlx::query_scalar("SELECT user_id FROM ws_connections WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(user_id)
}

/// Every `apigw` connection id `user_id` has open.
pub async fn for_user(db: &Db, user_id: &str) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM ws_connections WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(ids)
}

/// Same as the prev function, but with mutiple users
pub async fn for_users(db: &Db, user_ids: &[String]) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM ws_connections WHERE user_id = ANY($1)")
            .bind(user_ids)
            .fetch_all(pool)
            .await?;
    Ok(ids)
}

/// True if `user_id` has at least one `apigw` connection open.
pub async fn is_connected(db: &Db, user_id: &str) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let connected: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ws_connections WHERE user_id = $1)")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(connected)
}
