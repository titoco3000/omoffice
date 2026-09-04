//! The people who have asked for a key.
//!
//! `users` is keyed by the email `/auth/request-key/{email}` issues tokens for.
//! `auth.rs` writes the row when it signs a token, `meetings.created_by`
//! references it. There is no read path yet, so there is no row type here —
//! add one when something needs to list users.
//!
//! The SQL lives here rather than in `db.rs` on purpose: `db.rs` owns the
//! connection, this module owns the table.

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
