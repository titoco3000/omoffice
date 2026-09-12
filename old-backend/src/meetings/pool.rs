//! The reusable pool of live Chime meetings: `chime_meeting_pool` rows, and
//! the claim logic that hands an idle one to a different `meetings.id`
//! instead of minting a new Chime meeting every time. Knows nothing about
//! routes or org membership — `meetings/mod.rs` is the only caller besides
//! `presence`, which calls [`note_occupancy`].
//!
//! Which live Chime meeting backs a `meetings` row lives entirely here, in
//! `assigned_to` — nothing on `meetings` itself changes when the pool
//! reassigns it. "Idle" means `idle_since` is set and old enough:
//! `presence` already recomputes a meeting's viewer count on every join,
//! leave and disconnect, so [`note_occupancy`] just rides that same
//! recompute to stamp the moment a meeting drops under 2 viewers (cleared
//! the moment it climbs back) — no separate tracking of who's actually
//! connected to Chime, and no scheduled recheck: a claim simply compares
//! `idle_since` against the threshold, lazily, whenever one happens.
//!
//! Every write below that assigns a row to a `code` is guarded by "no other
//! row is already assigned to it", checked and written in the same
//! statement — so two callers racing to be the first to connect the same
//! brand-new meeting (both detecting "someone else just showed up" at
//! once) can't each mint or claim a *different* Chime meeting for it. DSQL
//! has no row locks, but it is serializable: the guard's read and the
//! write landing in one statement is what makes a racing pair conflict
//! (via [`retry_on_conflict`]) instead of both silently succeeding.

use aws_sdk_chimesdkmeetings::types::Meeting;

use crate::core::chime::{ChimeError, ChimeState};
use crate::core::db::{retry_on_conflict, Db, DbError};

/// How long a meeting has to sit under 2 viewers before its Chime room is
/// fair game for someone else — long enough that a quick reconnect (a
/// refresh, a dropped wifi bar) doesn't cost a fresh session.
const IDLE_AFTER_SECONDS: i64 = 20;

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

/// Tell the pool `code`'s viewer count just changed -- called by `presence`
/// on every recompute, not read back by anything here. Stamps `idle_since`
/// the moment a meeting first drops under 2 viewers (left alone on a
/// repeat call, so the window doesn't keep sliding), clears it the moment
/// it climbs back to 2 or more.
pub async fn note_occupancy(db: &Db, code: &str, viewer_count: usize) -> Result<(), DbError> {
    let pool = db.pool().await?;
    if viewer_count >= 2 {
        retry_on_conflict(|| async move {
            sqlx::query(
                "UPDATE chime_meeting_pool SET idle_since = NULL
                 WHERE assigned_to = $1 AND idle_since IS NOT NULL",
            )
            .bind(code)
            .execute(pool)
            .await
            .map(|_| ())
        })
        .await
    } else {
        retry_on_conflict(|| async move {
            sqlx::query(
                "UPDATE chime_meeting_pool SET idle_since = now()
                 WHERE assigned_to = $1 AND idle_since IS NULL",
            )
            .bind(code)
            .execute(pool)
            .await
            .map(|_| ())
        })
        .await
    }
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
/// none is idle — race-safe against another caller doing the same for the
/// same `code` at the same time; see the module doc.
async fn claim(db: &Db, chime: &ChimeState, code: &str) -> Result<(String, Meeting), PoolError> {
    if let Some(chime_id) = claim_idle(db, code).await? {
        return match chime.get_meeting(&chime_id).await {
            Ok(meeting) => Ok((chime_id, meeting)),
            Err(ChimeError::MeetingNotFound) => {
                tracing::warn!(
                    "pooled chime meeting {chime_id} was already gone; minting a new one"
                );
                forget(db, &chime_id).await.ok();
                mint(db, chime, code).await
            }
            Err(err) => Err(err.into()),
        };
    }

    // No idle row was ours to claim -- either the pool was genuinely empty,
    // or another caller's claim for this same `code` just won the race
    // `claim_idle`'s own guard lost us. Check for that before minting a
    // Chime meeting nobody would end up using.
    if let Some(chime_id) = current(db, code).await? {
        let meeting = chime.get_meeting(&chime_id).await?;
        return Ok((chime_id, meeting));
    }
    mint(db, chime, code).await
}

/// Mint a fresh Chime meeting and try to claim it for `code`. If another
/// caller wins `code` in the instant between that check and this insert,
/// this meeting is simply left for Chime to expire on its own -- nobody
/// will ever join it.
async fn mint(db: &Db, chime: &ChimeState, code: &str) -> Result<(String, Meeting), PoolError> {
    let meeting = chime.create_meeting().await?;
    let chime_id = meeting.meeting_id().unwrap_or_default().to_string();
    if insert_if_unclaimed(db, &chime_id, code).await? {
        return Ok((chime_id, meeting));
    }
    let winner = current(db, code)
        .await?
        .expect("insert_if_unclaimed lost, so a concurrent claim must have won `code`");
    let winner_meeting = chime.get_meeting(&winner).await?;
    Ok((winner, winner_meeting))
}

/// Reassign one idle, expired-idle pool row to `code` and hand back its
/// `chime_id` — atomic and guarded against another row already being
/// `code`'s, in one statement (see the module doc).
async fn claim_idle(db: &Db, code: &str) -> Result<Option<String>, DbError> {
    if let Err(err) = sweep_expired(db).await {
        tracing::warn!("could not sweep the chime meeting pool: {err:?}");
    }

    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query_scalar::<_, String>(
            "UPDATE chime_meeting_pool
             SET assigned_to = $1, idle_since = NULL
             WHERE id = (
                 SELECT p.id FROM chime_meeting_pool p
                 WHERE p.assigned_to IS NULL
                    OR (
                        p.idle_since IS NOT NULL
                        AND p.idle_since < now() - ($2 * interval '1 second')
                    )
                 ORDER BY p.created_at
                 LIMIT 1
             )
             AND NOT EXISTS (
                 SELECT 1 FROM chime_meeting_pool other WHERE other.assigned_to = $1
             )
             RETURNING id",
        )
        .bind(code)
        .bind(IDLE_AFTER_SECONDS)
        .fetch_optional(pool)
        .await
    })
    .await
}

/// Insert a brand-new pool row for `chime_id`, but only if `code` has no
/// row already — the fresh-mint counterpart to `claim_idle`'s guard.
/// Returns whether this call's `chime_id` won.
async fn insert_if_unclaimed(db: &Db, chime_id: &str, code: &str) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let inserted = retry_on_conflict(|| async move {
        sqlx::query_scalar::<_, String>(
            "INSERT INTO chime_meeting_pool (id, assigned_to)
             SELECT $1, $2
             WHERE NOT EXISTS (
                 SELECT 1 FROM chime_meeting_pool WHERE assigned_to = $2
             )
             RETURNING id",
        )
        .bind(chime_id)
        .bind(code)
        .fetch_optional(pool)
        .await
    })
    .await?;
    Ok(inserted.is_some())
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
