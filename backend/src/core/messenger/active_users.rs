//! The `active_users` table: one row per open socket — who is on the other
//! end, over which transport, and the context they last reported (which org
//! they are looking at, which meeting they are in). Only [`super::Messenger`]
//! reads or writes it; see `schema::ACTIVE_USERS`.

use crate::core::db::{retry_on_conflict, Db, DbError};

/// The `transport` column: an in-process socket held by this server.
pub const LOCAL: &str = "local";
/// The `transport` column: a WebSocket API connection, written to through
/// the Management API.
pub const APIGW: &str = "apigw";

/// One open socket.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActiveUser {
    pub connection_id: String,
    pub user_email: String,
    pub transport: String,
    pub org_id: Option<String>,
    pub meeting: Option<String>,
}

/// Record a newly accepted socket. Idempotent on `connection_id`, so a
/// repeat `$connect` is a no-op. Context starts empty — the client reports
/// it afterwards, over the socket itself.
pub async fn record(
    db: &Db,
    connection_id: &str,
    user_email: &str,
    transport: &str,
) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO active_users (connection_id, user_email, transport)
             VALUES ($1, $2, $3)
             ON CONFLICT (connection_id) DO NOTHING",
        )
        .bind(connection_id)
        .bind(user_email)
        .bind(transport)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await
}

/// Forget a socket: `$disconnect`, a closed local socket, or a lazy cleanup
/// after a push came back `Gone`. Removing an already-gone row is not an error.
pub async fn remove(db: &Db, connection_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM active_users WHERE connection_id = $1")
            .bind(connection_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Replace one connection's reported context. Both halves are always
/// written: a context report is the whole context, not a patch (see
/// [`super::Messenger::set_context`]).
pub async fn set_context(
    db: &Db,
    connection_id: &str,
    org_id: Option<&str>,
    meeting: Option<&str>,
) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE active_users SET org_id = $2, meeting = $3 WHERE connection_id = $1")
            .bind(connection_id)
            .bind(org_id)
            .bind(meeting)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Forget every socket on `transport`. Called once at local startup: a
/// process that just booted holds no sockets, so any `local` row left in
/// the table is from a run that did not close them (a Ctrl-C, a crash).
/// One cluster is shared by every local run of a stage, so this clears
/// another developer's live local sockets too -- they reconnect, and the
/// alternative is a table that only ever grows.
pub async fn clear_transport(db: &Db, transport: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM active_users WHERE transport = $1")
            .bind(transport)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Look a socket up by the id its own event carried — how an incoming
/// message learns who sent it and what context they are in.
pub async fn by_connection_id(db: &Db, connection_id: &str) -> Result<Option<ActiveUser>, DbError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, ActiveUser>(
        "SELECT connection_id, user_email, transport, org_id, meeting
         FROM active_users WHERE connection_id = $1",
    )
    .bind(connection_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Every socket `email` has open, in any context or none.
pub async fn for_user(db: &Db, email: &str) -> Result<Vec<ActiveUser>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, ActiveUser>(
        "SELECT connection_id, user_email, transport, org_id, meeting
         FROM active_users WHERE user_email = $1",
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every socket whose reported context is `org_id`.
pub async fn in_org(db: &Db, org_id: &str) -> Result<Vec<ActiveUser>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, ActiveUser>(
        "SELECT connection_id, user_email, transport, org_id, meeting
         FROM active_users WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every socket whose reported context is meeting `code`.
pub async fn in_meeting(db: &Db, code: &str) -> Result<Vec<ActiveUser>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, ActiveUser>(
        "SELECT connection_id, user_email, transport, org_id, meeting
         FROM active_users WHERE meeting = $1",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
