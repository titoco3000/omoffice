//! The reusable pool of live Chime meetings: `chime_meeting_pool` rows, and
//! the claim logic that hands an idle one to a different `meetings.id`
//! instead of minting a new Chime meeting every time. Knows nothing about
//! routes or org membership — `meetings/mod.rs` is the only caller.
//!
//! Which live Chime meeting backs a `meetings` row lives entirely here, in
//! `assigned_to` — nothing on `meetings` itself changes when the pool
//! reassigns it. "Idle" is read off `active_users`: a row is claimable once
//! nobody's `active_users.meeting` still names its owner, or it was never
//! assigned at all. There's no minimum wait after that — a claim that turns
//! out to name a Chime meeting already gone is just
//! [`ChimeError::MeetingNotFound`], handled the same as any other miss, by
//! minting a fresh one.

use aws_sdk_chimesdkmeetings::types::Meeting;

use crate::core::chime::{ChimeError, ChimeState};
use crate::core::db::{retry_on_conflict, Db, DbError};

/// A live Chime meeting for `code`: whichever one the pool currently has
/// assigned to it if that's still live, else an idle pooled one, else a
/// freshly minted one. Either way, the returned `chime_id`'s pool row ends
/// up owned by `code` — so `create` (nothing assigned yet) and `join`
/// (resuming, or recovering from a reassignment) both just call this.
pub async fn live(db: &Db, chime: &ChimeState, code: &str) -> Result<(String, Meeting), PoolError> {
    if let Some(chime_id) = current(db, code).await? {
        match chime.get_meeting(&chime_id).await {
            Ok(meeting) => return Ok((chime_id, meeting)),
            Err(ChimeError::MeetingNotFound) => {
                // Our bookkeeping thought it was still ours and alive;
                // Chime had already dropped it. Forget the row and fall
                // through to claiming a different one.
                tracing::warn!("chime meeting {chime_id} for {code} was already gone");
                forget(db, &chime_id).await.ok();
            }
            Err(err) => return Err(err.into()),
        }
    }
    claim(db, chime, code).await
}

/// The `chime_id` the pool currently has assigned to `code`, if any.
async fn current(db: &Db, code: &str) -> Result<Option<String>, DbError> {
    let pool = db.pool().await?;
    let chime_id = sqlx::query_scalar("SELECT id FROM chime_meeting_pool WHERE assigned_to = $1")
        .bind(code)
        .fetch_optional(pool)
        .await?;
    Ok(chime_id)
}

/// Reassign one idle pool row to `code`, or mint a fresh Chime meeting if
/// none is idle.
async fn claim(db: &Db, chime: &ChimeState, code: &str) -> Result<(String, Meeting), PoolError> {
    if let Some(chime_id) = claim_idle(db, code).await? {
        match chime.get_meeting(&chime_id).await {
            Ok(meeting) => return Ok((chime_id, meeting)),
            Err(ChimeError::MeetingNotFound) => {
                tracing::warn!(
                    "pooled chime meeting {chime_id} was already gone; minting a new one"
                );
                forget(db, &chime_id).await.ok();
            }
            Err(err) => return Err(err.into()),
        }
    }

    let meeting = chime.create_meeting().await?;
    let chime_id = meeting.meeting_id().unwrap_or_default().to_string();
    insert(db, &chime_id, code).await?;
    Ok((chime_id, meeting))
}

/// Reassign one idle, non-expired pool row to `code` and hand back its
/// `chime_id` — atomic, so two callers racing for the same idle row never
/// both win it (DSQL has no row locks; a losing writer just retries).
async fn claim_idle(db: &Db, code: &str) -> Result<Option<String>, DbError> {
    if let Err(err) = sweep_expired(db).await {
        tracing::warn!("could not sweep the chime meeting pool: {err:?}");
    }

    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query_scalar::<_, String>(
            "UPDATE chime_meeting_pool
             SET assigned_to = $1
             WHERE id = (
                 SELECT p.id FROM chime_meeting_pool p
                 WHERE p.assigned_to IS NULL
                    OR NOT EXISTS (
                        SELECT 1 FROM active_users a WHERE a.meeting = p.assigned_to
                    )
                 ORDER BY p.created_at
                 LIMIT 1
             )
             RETURNING id",
        )
        .bind(code)
        .fetch_optional(pool)
        .await
    })
    .await
}

/// Drop rows older than 12h, claimed or not — comfortably inside Chime's
/// own 24h hard cap, so nothing pool-claimed is ever close to Chime killing
/// it out from under its new owner. "When the system is looking for
/// meetings in the pool, it removes expired meetings."
async fn sweep_expired(db: &Db) -> Result<(), DbError> {
    let pool = db.pool().await?;
    sqlx::query("DELETE FROM chime_meeting_pool WHERE created_at < now() - interval '12 hours'")
        .execute(pool)
        .await?;
    Ok(())
}

async fn insert(db: &Db, chime_id: &str, code: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO chime_meeting_pool (id, assigned_to) VALUES ($1, $2)
             ON CONFLICT (id) DO UPDATE SET assigned_to = excluded.assigned_to",
        )
        .bind(chime_id)
        .bind(code)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await
}

async fn forget(db: &Db, chime_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    sqlx::query("DELETE FROM chime_meeting_pool WHERE id = $1")
        .bind(chime_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// What claiming a pool slot can fail with — folded into `MeetingError` by
/// `meetings/mod.rs`, the same way it folds `RbacError`.
#[derive(Debug)]
pub enum PoolError {
    Chime(ChimeError),
    Database(DbError),
}

impl From<ChimeError> for PoolError {
    fn from(err: ChimeError) -> Self {
        Self::Chime(err)
    }
}

impl From<DbError> for PoolError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}
