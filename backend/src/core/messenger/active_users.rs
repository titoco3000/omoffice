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

/// How long a row is trusted as "still connected" without a fresh [`touch`].
const FRESH_FOR_SECONDS: i32 = 60;

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
            "INSERT INTO active_users (connection_id, user_email, transport, last_seen_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (connection_id) DO NOTHING",
        )
        .bind(connection_id)
        .bind(user_email)
        .bind(transport)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;

    // Best effort, and not worth failing a connect over: a new socket is
    // as good a moment as any to sweep whatever `$disconnect` never did.
    if let Err(err) = reap_stale(db).await {
        tracing::warn!("messenger: could not reap stale connections: {err:?}");
    }
    Ok(())
}

/// Forget a socket: `$disconnect`, a closed local socket, or a lazy cleanup
/// after a push came back `Gone`. Removing an already-gone row is not an
/// error. Returns the org it had reported, if any and if there was a row at
/// all -- `Messenger::forget` uses that to tell `presence` an org's roster
/// just shrank.
pub async fn remove(db: &Db, connection_id: &str) -> Result<Option<String>, DbError> {
    let pool = db.pool().await?;
    let org_id = retry_on_conflict(|| async move {
        sqlx::query_scalar::<_, Option<String>>(
            "DELETE FROM active_users WHERE connection_id = $1 RETURNING org_id",
        )
        .bind(connection_id)
        .fetch_optional(pool)
        .await
    })
    .await?;
    Ok(org_id.flatten())
}

/// Mark a socket as still alive. Called on every inbound frame
/// (`Messenger::dispatch`), including the client's idle heartbeat, since a
/// frame arriving at all is proof the socket is still open -- cheaper than
/// waiting for a context change, which may never come.
pub async fn touch(db: &Db, connection_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE active_users SET last_seen_at = now() WHERE connection_id = $1")
            .bind(connection_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Delete rows nobody has touched in a long time (well past
/// [`FRESH_FOR_SECONDS`] -- a generous multiple, not the same cutoff a
/// reader uses). A `$disconnect` that never fires otherwise leaves the row
/// forever; called opportunistically on `record` so the table does not
/// grow without bound from connections nothing will ever clean up.
pub async fn reap_stale(db: &Db) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM active_users WHERE last_seen_at < now() - interval '1 hour'")
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

/// Every socket whose reported context is `org_id` and that has been
/// touched within [`FRESH_FOR_SECONDS`] -- a row `$disconnect` never
/// cleared is not "here" just because it still exists.
pub async fn in_org(db: &Db, org_id: &str) -> Result<Vec<ActiveUser>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, ActiveUser>(
        "SELECT connection_id, user_email, transport, org_id, meeting
         FROM active_users
         WHERE org_id = $1 AND last_seen_at > now() - ($2 * interval '1 second')",
    )
    .bind(org_id)
    .bind(FRESH_FOR_SECONDS)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every socket whose reported context is meeting `code` and that has been
/// touched within [`FRESH_FOR_SECONDS`]; see [`in_org`].
pub async fn in_meeting(db: &Db, code: &str) -> Result<Vec<ActiveUser>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, ActiveUser>(
        "SELECT connection_id, user_email, transport, org_id, meeting
         FROM active_users
         WHERE meeting = $1 AND last_seen_at > now() - ($2 * interval '1 second')",
    )
    .bind(code)
    .bind(FRESH_FOR_SECONDS)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
