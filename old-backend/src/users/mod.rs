//! The people who have asked for a key. Keyed by the email
//! `/auth/request-key/{email}` issues tokens for; no read path yet, so no row type here.

use crate::core::db::{retry_on_conflict, Db, DbError};

/// Record the email a token was just issued to. Idempotent: the same person
/// asks for a key many times.
pub async fn record(db: &Db, email: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("INSERT INTO users (email) VALUES ($1) ON CONFLICT (email) DO NOTHING")
            .bind(email)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Is `email` already a row here? The invite flow uses this to pick which
/// of the two email templates to send.
pub async fn exists(db: &Db, email: &str) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let row: Option<i32> = sqlx::query_scalar("SELECT 1 FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}
