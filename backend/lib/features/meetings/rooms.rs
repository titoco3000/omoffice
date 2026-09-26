//! The reusable pool of live Chime rooms (`meeting_rooms` rows) and the claim
//! logic that hands an idle one to a different meeting instead of minting a
//! fresh Chime session every time. Knows nothing about routes, org
//! membership, or how many people are actually in a meeting -- that is
//! `mod.rs`'s job; this module only ever moves a room between "assigned to
//! this meeting" and "free for the next one that needs it".
//!
//! Two different things free a room, keyed two different ways:
//!
//! - Chime's own signal that a room emptied out ([`ChimeEvent::MeetingEmptied`],
//!   wired up by [`register`]) is keyed by the Chime room id -- Chime has no
//!   idea which of our meetings that room backs.
//! - `mod.rs`'s own bookkeeping (someone left, leaving the meeting with
//!   fewer than two attendees) is keyed by our own meeting id instead, via
//!   [`release`] -- proactive, not waited on: a two-person room with one
//!   person left in it is freed immediately rather than left assigned until
//!   Chime notices.
//!
//! [`assign`] always double-checks an already-assigned room is still
//! reachable before handing it back out (a stale `assigned_to` surviving a
//! Chime-side drop we haven't heard about yet would otherwise hand a caller
//! a room nobody can actually join). There is no explicit "destroy" either:
//! a meeting ending just [`release`]s its room like any other drop below two
//! attendees, and a room nobody ever reclaims quietly ages out via
//! [`claim_idle`]'s own sweep, or gets forgotten the moment some future
//! `assign` finds Chime already dropped it.

use aws_sdk_chimesdkmeetings::types::Meeting;

use crate::core::chime::{ChimeError, ChimeEvent, ChimeState};
use crate::core::data::db::{retry_on_conflict, Db, DbError};

/// Unassigned rows older than this are dropped rather than ever handed out
/// -- comfortably inside Chime's own 24h hard cap on a meeting's lifetime,
/// so nothing pool-claimed is ever close to Chime killing it out from under
/// whoever just got handed it.
const MAX_ROOM_AGE_HOURS: i64 = 12;

#[derive(Debug)]
pub enum RoomError {
    Chime(ChimeError),
    Database(DbError),
}

impl From<ChimeError> for RoomError {
    fn from(err: ChimeError) -> Self {
        Self::Chime(err)
    }
}

impl From<DbError> for RoomError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

/// Wire the pool up to Chime's own signal that a room emptied out. Meant to
/// be called once, at startup.
pub fn register(db: Db, chime: &ChimeState) {
    chime.on_event(move |event| {
        let ChimeEvent::MeetingEmptied {
            meeting_id: room_id,
        } = event;
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(err) = release_room(&db, &room_id).await {
                tracing::warn!("rooms: could not free chime room {room_id}: {err:?}");
            }
        });
    });
}

/// The live Chime room for `meeting_id`: whichever one is already assigned
/// to it if that's still live, else an idle pooled one, else a freshly
/// minted one. The `bool` says whether this call actually claimed a room
/// (fresh, or recovered from one Chime had dropped) rather than simply
/// confirming the one `meeting_id` already had -- `mod.rs::join` uses that
/// to know whether anyone already connected needs reconnecting to a new
/// room, or is fine where they are.
///
/// Guarded against two callers racing to claim a room for the same
/// `meeting_id` at once: DSQL is serializable rather than lock-based, so the
/// guard's read and its write must land in one statement for a racing pair
/// to conflict instead of both silently succeeding.
pub async fn assign(
    db: &Db,
    chime: &ChimeState,
    meeting_id: &str,
) -> Result<(String, Meeting, bool), RoomError> {
    if let Some(room_id) = current(db, meeting_id).await? {
        match chime.get_meeting(&room_id).await {
            Ok(meeting) => return Ok((room_id, meeting, false)),
            Err(ChimeError::MeetingNotFound) => {
                // Our bookkeeping thought this was still live; Chime had
                // already dropped it. Forget the row and fall through to
                // claiming a different one.
                tracing::warn!("chime room {room_id} for meeting {meeting_id} was already gone");
                forget(db, &room_id).await.ok();
            }
            Err(err) => return Err(err.into()),
        }
    }
    let (room_id, meeting) = claim(db, chime, meeting_id).await?;
    Ok((room_id, meeting, true))
}

/// Free `meeting_id`'s room back to the pool for reuse -- called once its
/// attendee count drops under two, not waited on for Chime to notice.
pub(crate) async fn release(db: &Db, meeting_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE meeting_rooms SET assigned_to = NULL WHERE assigned_to = $1")
            .bind(meeting_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Free whichever meeting is holding `room_id` back to the pool -- called
/// when Chime itself reports that room emptied out, keyed by the Chime room
/// id rather than our own meeting id (Chime has no idea which of our
/// meetings a room backs).
async fn release_room(db: &Db, room_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE meeting_rooms SET assigned_to = NULL WHERE id = $1")
            .bind(room_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// The room id currently assigned to `meeting_id`, if any.
async fn current(db: &Db, meeting_id: &str) -> Result<Option<String>, DbError> {
    let pool = db.pool().await?;
    let room_id = sqlx::query_scalar("SELECT id FROM meeting_rooms WHERE assigned_to = $1")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;
    Ok(room_id)
}

/// Reassign one idle pool row to `meeting_id`, or mint a fresh Chime meeting
/// if none is idle -- race-safe against another caller doing the same for
/// the same `meeting_id` at the same time; see the module doc.
async fn claim(
    db: &Db,
    chime: &ChimeState,
    meeting_id: &str,
) -> Result<(String, Meeting), RoomError> {
    if let Some(room_id) = claim_idle(db, meeting_id).await? {
        return match chime.get_meeting(&room_id).await {
            Ok(meeting) => Ok((room_id, meeting)),
            Err(ChimeError::MeetingNotFound) => {
                tracing::warn!("pooled chime room {room_id} was already gone; minting a new one");
                forget(db, &room_id).await.ok();
                mint(db, chime, meeting_id).await
            }
            Err(err) => Err(err.into()),
        };
    }

    // No idle row was ours to claim -- either the pool was genuinely empty,
    // or another caller's claim for this same `meeting_id` just won the
    // race `claim_idle`'s own guard lost us. Check for that before minting
    // a Chime meeting nobody would end up using.
    if let Some(room_id) = current(db, meeting_id).await? {
        let meeting = chime.get_meeting(&room_id).await?;
        return Ok((room_id, meeting));
    }
    mint(db, chime, meeting_id).await
}

/// Mint a fresh Chime meeting and try to claim it for `meeting_id`. If
/// another caller wins `meeting_id` in the instant between that check and
/// this insert, this meeting is simply left for Chime to expire on its own
/// -- nobody will ever join it.
async fn mint(
    db: &Db,
    chime: &ChimeState,
    meeting_id: &str,
) -> Result<(String, Meeting), RoomError> {
    let meeting = chime.create_meeting().await?;
    let room_id = meeting.meeting_id().unwrap_or_default().to_string();
    if insert_if_unclaimed(db, &room_id, meeting_id).await? {
        return Ok((room_id, meeting));
    }
    let winner = current(db, meeting_id)
        .await?
        .expect("insert_if_unclaimed lost, so a concurrent claim must have won `meeting_id`");
    let winner_meeting = chime.get_meeting(&winner).await?;
    Ok((winner, winner_meeting))
}

/// Reassign one idle pool row to `meeting_id` and hand back its room id --
/// atomic and guarded against another row already being `meeting_id`'s, in
/// one statement (see the module doc). Also sweeps rooms too old to trust,
/// in the background: that sweep is housekeeping, not something this claim
/// should wait a round trip on. Claiming one it was about to sweep is
/// harmless -- a room Chime already dropped is noticed and replaced by
/// [`claim`] either way.
async fn claim_idle(db: &Db, meeting_id: &str) -> Result<Option<String>, DbError> {
    {
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(err) = sweep_expired(&db).await {
                tracing::warn!("rooms: could not sweep expired pool rows: {err:?}");
            }
        });
    }

    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query_scalar::<_, String>(
            "UPDATE meeting_rooms
             SET assigned_to = $1
             WHERE id = (
                 SELECT id FROM meeting_rooms WHERE assigned_to IS NULL
                 ORDER BY created_at LIMIT 1
             )
             AND NOT EXISTS (
                 SELECT 1 FROM meeting_rooms other WHERE other.assigned_to = $1
             )
             RETURNING id",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    })
    .await
}

/// Insert a brand-new pool row for `room_id`, but only if `meeting_id` has
/// no row already -- the fresh-mint counterpart to `claim_idle`'s guard.
/// Returns whether this call's `room_id` won.
async fn insert_if_unclaimed(db: &Db, room_id: &str, meeting_id: &str) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let inserted = retry_on_conflict(|| async move {
        sqlx::query_scalar::<_, String>(
            "INSERT INTO meeting_rooms (id, assigned_to)
             SELECT $1, $2
             WHERE NOT EXISTS (
                 SELECT 1 FROM meeting_rooms WHERE assigned_to = $2
             )
             RETURNING id",
        )
        .bind(room_id)
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    })
    .await?;
    Ok(inserted.is_some())
}

/// Drop unassigned rows older than [`MAX_ROOM_AGE_HOURS`] -- assigned rows
/// are left alone even past that age; only a pooled, idle room is ever
/// silently forgotten.
async fn sweep_expired(db: &Db) -> Result<(), DbError> {
    let pool = db.pool().await?;
    sqlx::query(
        "DELETE FROM meeting_rooms
         WHERE assigned_to IS NULL
           AND created_at < now() - ($1 * interval '1 hour')",
    )
    .bind(MAX_ROOM_AGE_HOURS)
    .execute(pool)
    .await?;
    Ok(())
}

async fn forget(db: &Db, room_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    sqlx::query("DELETE FROM meeting_rooms WHERE id = $1")
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}
