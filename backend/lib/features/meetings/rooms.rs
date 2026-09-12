//! The reusable pool of live Chime rooms (`meeting_rooms` rows) and the claim
//! logic that hands an idle one to a different meeting instead of minting a
//! fresh Chime session every time. Knows nothing about routes or org
//! membership.
//!
//! Freeing a room does not come from polling occupancy: [`register`]
//! subscribes to [`ChimeEvent::MeetingEmptied`] and clears `assigned_to`
//! right then. That is also why there is no presence/active-users table in
//! this overhaul: emptiness is Chime's own fact to report, not something we
//! have to reconstruct by watching who is connected.

use aws_sdk_chimesdkmeetings::types::Meeting;

use crate::core::chime::{ChimeError, ChimeEvent, ChimeState};
use crate::core::data::db::{Db, DbError};

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
        let ChimeEvent::MeetingEmptied { meeting_id } = event;
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(err) = release(&db, &meeting_id).await {
                tracing::warn!("rooms: could not free {meeting_id}: {err:?}");
            }
        });
    });
}

/// The live Chime room for `meeting_id`: whichever one is already assigned
/// to it if that's still live, else an idle pooled one, else a freshly
/// minted one. Guarded against two callers racing to claim a room for the
/// same `meeting_id` at once: DSQL is serializable rather than lock-based,
/// so the guard's read and its write must land in one statement for a
/// racing pair to conflict instead of both silently succeeding.
pub async fn assign(
    db: &Db,
    chime: &ChimeState,
    meeting_id: &str,
) -> Result<(String, Meeting), RoomError> {
    let _ = (db, chime, meeting_id);
    todo!()
}

/// Free `meeting_id`'s room back to the pool -- called when Chime reports it
/// emptied, not by anything that watches occupancy itself.
async fn release(db: &Db, meeting_id: &str) -> Result<(), DbError> {
    let _ = (db, meeting_id);
    todo!()
}
