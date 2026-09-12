//! Meetings: the Chime session behind one, and the routes. The only feature
//! that knows both [`crate::core::chime`] and [`crate::core::data::db`] for
//! a meeting -- [`chat`], [`meeting_invites`] and [`rooms`] are split out
//! because each is a smaller, separately reviewable slice of that same idea.
//!
//! Joining is two steps behind one call: [`join`] asks [`rooms::assign`] for
//! a live room (assigning one if none is held yet), then mints a Chime
//! attendee in it. Freeing a room back up happens the other way around --
//! see `rooms`' own doc for why nothing here has to ask for that to happen.

pub mod chat;
pub mod meeting_invites;
pub mod rooms;
pub use meeting_invites::*;

use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::core::chime::{self, ChimeError, ChimeState};
use crate::core::data::db::{Db, DbError};
use crate::core::rbac::RbacError;
use rooms::RoomError;

/// One row of `meetings`.
#[derive(Debug)]
pub struct Meeting {
    pub id: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub org_id: Option<String>,
    pub invite_only: bool,
}

#[derive(Debug)]
pub enum MeetingError {
    Chime(ChimeError),
    Database(DbError),
    Rbac(RbacError),
    Room(RoomError),
    /// No `meetings` row for that id.
    NotFound,
    /// Caller isn't the meeting's creator.
    Forbidden,
    InviteNotFound,
    InviteNotPending,
}

impl From<ChimeError> for MeetingError {
    fn from(err: ChimeError) -> Self {
        Self::Chime(err)
    }
}

impl From<DbError> for MeetingError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

impl From<RbacError> for MeetingError {
    fn from(err: RbacError) -> Self {
        Self::Rbac(err)
    }
}

impl From<RoomError> for MeetingError {
    fn from(err: RoomError) -> Self {
        Self::Room(err)
    }
}

/// Create a meeting under `org_id`. No room is claimed yet -- a meeting with
/// nobody in it costs nothing until [`join`] actually needs one.
pub async fn create(
    db: &Db,
    created_by: &str,
    org_id: Option<&str>,
    invite_only: bool,
) -> Result<Meeting, MeetingError> {
    let _ = (db, created_by, org_id, invite_only);
    todo!()
}

/// Join a meeting: get its room via [`rooms::assign`] (claiming one if it
/// has none), then mint a Chime attendee in it. Returns the
/// `Meeting`/`Attendee` JSON the browser's Chime SDK expects -- see
/// [`chime::meeting_json`]/[`chime::attendee_json`].
pub async fn join(
    db: &Db,
    chime: &ChimeState,
    meeting_id: &str,
    user_id: &str,
) -> Result<Value, MeetingError> {
    let (room_id, session) = rooms::assign(db, chime, meeting_id).await?;
    let attendee = chime
        .create_attendee(&room_id, &chime::external_user_id(user_id))
        .await?;
    Ok(serde_json::json!({
        "meeting_id": meeting_id,
        "Meeting": chime::meeting_json(&session),
        "Attendee": chime::attendee_json(&attendee),
    }))
}

/// Every meeting in `org_id`, newest first -- any member sees and can rejoin
/// any of them, not just their own.
pub async fn list(db: &Db, org_id: &str) -> Result<Vec<Meeting>, MeetingError> {
    let _ = (db, org_id);
    todo!()
}

/// `/meeting/*` -- create/join/list, plus chat, invites and room status
/// nested in from the sibling modules. Empty for now: `create`/`join`/`list`
/// above and `rooms::assign` are still `todo!()`, so there is nothing to
/// route to yet that wouldn't panic on the first request. An empty router
/// 404s instead, so `router::app` builds and the rest of the server -- rbac
/// included -- is reachable and testable ahead of phase 14.3.
pub fn routes() -> Router {
    Router::new()
}
