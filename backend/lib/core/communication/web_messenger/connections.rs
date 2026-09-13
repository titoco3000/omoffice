//! The `ws_connections` table: one row per open socket -- who is on the
//! other end. Only [`super::Messenger`] reads or writes it. This is the
//! single roster both a local run and a deployed Lambda resolve "who is
//! user X's socket(s)" through, so `Messenger::send` and `is_connected`
//! behave the same in either mode, and a Lambda invocation that shares no
//! memory with the one before it still finds every connection a user has
//! open.
//!
//! Which transport actually holds a given connection is not stored here --
//! [`super::web_sockets::Hub::send`] answers that at delivery time instead,
//! so there is no separate label that could drift out of sync with reality.
//! Local ids are prefixed `local-` (see [`super::upgrade_route`]) purely so
//! [`clear_transport`] can find them at startup; nothing else depends on
//! the prefix.

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

/// Forget a socket: `$disconnect`, a closed local socket, or a lazy cleanup
/// after a push came back `Gone`. Removing an already-gone row is not an
/// error. Returns the user it belonged to, if there was a row at all --
/// [`super::Messenger::forget`] uses that to fire [`super::DISCONNECTED`]
/// on [`super::CONNECTION`].
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

/// Every socket id `user_id` has open.
pub async fn for_user(db: &Db, user_id: &str) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM ws_connections WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(ids)
}

/// True if `user_id` has at least one row.
pub async fn is_connected(db: &Db, user_id: &str) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let connected: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ws_connections WHERE user_id = $1)")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(connected)
}

/// Forget every local socket, told apart by their `local-` id prefix (see
/// the module doc). Called once at local startup: a process that just
/// booted holds no sockets, so any such row left in the table is from a run
/// that did not close them cleanly (a Ctrl-C, a crash). One cluster is
/// shared by every local run of a stage, so this also clears another
/// developer's live local sockets -- they reconnect, and the alternative is
/// a table that only ever grows.
pub async fn clear_local(db: &Db) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM ws_connections WHERE id LIKE 'local-%'")
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}
