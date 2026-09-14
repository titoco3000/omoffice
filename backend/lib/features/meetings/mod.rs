//! Meetings: the Chime session behind one, and the routes. The only feature
//! that knows both [`crate::core::chime`] and [`crate::core::data::db`] for
//! a meeting -- [`chat`], [`meeting_invites`] and [`rooms`] are split out
//! because each is a smaller, separately reviewable slice of that same idea.
//!
//! A meeting is created with no room and, in the frontend, no Chime
//! connection either -- the creator just appears "in" it locally. Nothing
//! costs anything (no room, no attendee count worth tracking) until a
//! second person actually shows up: [`join`] is what claims a room, via
//! [`rooms::assign`], the first time a meeting's attendee count reaches two.
//! A solo `join` (the only person present, whether the creator rejoining
//! alone or the last person left) claims no room either, and gets told so
//! in its own response, same as creation.
//!
//! Below two attendees, a meeting is "idle" -- `idle_since` records the
//! moment it dropped there. Nothing watches the clock on its own: there is
//! no background task. Instead, whoever next touches an idle-past-
//! [`IDLE_AFTER_SECONDS`] meeting is what terminates it -- [`list_route`]
//! sweeps its org before answering, and [`join`] sweeps the one meeting it
//! was asked for before deciding whether to let the caller in. The
//! frontend is trusted to do the same thing from its side: once it notices
//! it's been alone past the same window, it just calls `leave` on itself.
//! That trust ends the moment the tab is simply closed rather than
//! navigated away from -- no request, no unmount, nothing to call `leave`.
//! [`register`] is the backstop: it shares [`web_messenger::CONNECTION`]
//! with `core::rbac`'s own subscriber (see that module's doc) and calls
//! `leave` on a user's behalf, for every meeting it finds them still an
//! attendee of, the moment their socket actually drops.
//!
//! [`ROOM_CHANNEL`] is how a room reaches someone who didn't just call
//! `join` themselves: whoever else is already in a meeting when [`join`]
//! claims a *new* room for it (as opposed to reusing one still live) gets
//! reconnected over it, and [`leave`]/[`sweep_idle`] use it the other way,
//! to say a room was cleared or the meeting ended. [`LIST_CHANNEL`] is
//! separate -- an org-wide feed of which meetings currently exist and who
//! is in each, pushed whenever one is created, terminated, joined or left.
//! [`meeting_invites::CHANNEL`] is separate again: a feed of one user's own
//! pending invites, not scoped to an org at all.

pub mod chat;
pub mod controls;
pub mod meeting_invites;
pub mod rooms;
pub use meeting_invites::*;

use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::core::auth::Claims;
use crate::core::chime::{self, ChimeError, ChimeState};
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::{retry_on_conflict, Db, DbError};
use crate::core::rbac::{self, RbacError};
use rooms::RoomError;

/// How long a meeting may sit at fewer than two attendees before the next
/// thing to touch it terminates it
const IDLE_AFTER_SECONDS: i64 = 60;

/// Pushes `{"org_id", "meetings": [MeetingWithAttendees, ...]}` to every
/// online member of an org whenever a meeting is created, terminated,
/// joined or left.
pub const LIST_CHANNEL: web_messenger::Channel = "meeting_list";

/// Pushes a room reconnect/disconnect to one meeting's attendees directly,
/// outside whatever HTTP call triggered it. Frame
/// shapes: `{"meeting_id", "connected": true, "Meeting", "Attendee"}` (a
/// fresh room to join), `{"meeting_id", "connected": false}` (the room was
/// cleared, but the meeting itself continues), or `{"meeting_id", "ended": true}`.
pub const ROOM_CHANNEL: web_messenger::Channel = "meeting_room";

/// Pushes a forced join straight to one user, outside whatever HTTP call
/// triggered it -- see [`force_join`]. Frame shape:
/// `{"meeting_id", "Meeting", "Attendee"}`. Unlike [`ROOM_CHANNEL`], there is
/// no accept step behind this: the frontend answers by connecting
/// immediately, muted and with the camera off.
pub const FORCE_CHANNEL: web_messenger::Channel = "meeting_force";

/// One row of `meetings`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Meeting {
    pub id: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub org_id: Option<String>,
    pub invite_only: bool,
    pub idle_since: Option<DateTime<Utc>>,
}

impl Meeting {
    /// Whether this meeting has sat idle long enough that whoever's holding
    /// it should terminate it rather than use it -- see the module doc.
    fn idle_expired(&self) -> bool {
        self.idle_since
            .is_some_and(|since| Utc::now() - since > Duration::seconds(IDLE_AFTER_SECONDS))
    }
}

/// A [`Meeting`] plus who's currently in it -- what [`list`] actually hands
/// out, so a roster elsewhere in the org can tell "in a meeting" from
/// "which one".
#[derive(Debug, Clone, Serialize)]
pub struct MeetingWithAttendees {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub attendees: Vec<String>,
    /// Whether this meeting currently has any outstanding invite -- see
    /// [`meeting_invites::create_invite`], which pauses `idle_since` while
    /// this is true so the backend itself never sweeps a solo meeting out
    /// from under a sent invite. Surfaced here too so the frontend's own
    /// local idle-leave timer (`useMeetingSession.js`, a proactive mirror
    /// of the same rule) can skip itself the same way, rather than
    /// dropping the tab out of a meeting the backend still considers live.
    pub pending_invites: bool,
}

#[derive(Debug)]
pub enum MeetingError {
    Chime(ChimeError),
    Database(DbError),
    Rbac(RbacError),
    Room(RoomError),
    /// No `meetings` row for that id, or one so idle it's treated the same.
    NotFound,
    /// Caller isn't the meeting's creator (creating/revoking an invite,
    /// joining an invite-only meeting with no pending invite of their own)
    /// or isn't who an invite was addressed to (declining someone else's).
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

/// So a plain `sqlx::query...(...).await?` composes inside a function
/// returning `Result<_, MeetingError>`, same as `RbacError`'s own impl.
impl From<sqlx::Error> for MeetingError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(DbError::from(err))
    }
}

impl IntoResponse for MeetingError {
    fn into_response(self) -> Response {
        match self {
            Self::Chime(err) => err.into_response(),
            Self::Database(err) => err.into_response(),
            Self::Rbac(err) => err.into_response(),
            Self::Room(RoomError::Chime(err)) => err.into_response(),
            Self::Room(RoomError::Database(err)) => err.into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "meeting_not_found", "message": "no such meeting" })),
            )
                .into_response(),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "meeting_forbidden",
                    "message": "you may not do that with this meeting or invite",
                })),
            )
                .into_response(),
            Self::InviteNotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "invite_not_found", "message": "no such invite" })),
            )
                .into_response(),
            Self::InviteNotPending => (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "invite_not_pending",
                    "message": "this invite has already been accepted, revoked or declined",
                })),
            )
                .into_response(),
        }
    }
}

/// Single-row fetch by id, for `join`/`leave`'s existence check.
async fn find(db: &Db, meeting_id: &str) -> Result<Option<Meeting>, DbError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, Meeting>(
        "SELECT id, created_by, created_at, org_id, invite_only, idle_since
         FROM meetings WHERE id = $1",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub(crate) async fn set_idle_since(
    db: &Db,
    meeting_id: &str,
    value: Option<DateTime<Utc>>,
) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE meetings SET idle_since = $1 WHERE id = $2")
            .bind(value)
            .bind(meeting_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

pub(crate) async fn attendee_count(db: &Db, meeting_id: &str) -> Result<i64, DbError> {
    let pool = db.pool().await?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM meeting_attendees WHERE meeting_id = $1")
            .bind(meeting_id)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

async fn attendee_ids(db: &Db, meeting_id: &str) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids = sqlx::query_scalar("SELECT user_id FROM meeting_attendees WHERE meeting_id = $1")
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
    Ok(ids)
}

/// Every meeting `user_id` is currently an attendee of -- for [`register`],
/// which needs to leave them out of all of them on a hard disconnect. In
/// practice the frontend keeps one tab in at most one meeting at a time, so
/// this is normally zero or one row, not a scan worth indexing further.
async fn attendee_meeting_ids(db: &Db, user_id: &str) -> Result<Vec<String>, DbError> {
    let pool = db.pool().await?;
    let ids = sqlx::query_scalar("SELECT meeting_id FROM meeting_attendees WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(ids)
}

async fn upsert_attendee(db: &Db, meeting_id: &str, user_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO meeting_attendees (meeting_id, user_id) VALUES ($1, $2)
             ON CONFLICT (meeting_id, user_id) DO UPDATE SET joined_at = now()",
        )
        .bind(meeting_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await
}

async fn remove_attendee(db: &Db, meeting_id: &str, user_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM meeting_attendees WHERE meeting_id = $1 AND user_id = $2")
            .bind(meeting_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

async fn delete_attendees(db: &Db, meeting_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM meeting_attendees WHERE meeting_id = $1")
            .bind(meeting_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

async fn delete_meeting_row(db: &Db, meeting_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM meetings WHERE id = $1")
            .bind(meeting_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Every meeting in `org_id` idle past [`IDLE_AFTER_SECONDS`], for
/// [`sweep_idle`].
async fn idle_expired_in_org(db: &Db, org_id: &str) -> Result<Vec<Meeting>, DbError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, Meeting>(
        "SELECT id, created_by, created_at, org_id, invite_only, idle_since
         FROM meetings
         WHERE org_id = $1
           AND idle_since IS NOT NULL
           AND idle_since < now() - ($2 * interval '1 second')",
    )
    .bind(org_id)
    .bind(IDLE_AFTER_SECONDS)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Create a meeting under `org_id`. No room is claimed yet, and no attendee
/// beyond the creator themself -- a meeting with nobody else in it costs
/// nothing until [`join`] actually needs a room. Starts idle: a lone
/// creator who never gets company is swept the same as anyone else left
/// alone.
pub async fn create(
    db: &Db,
    created_by: &str,
    org_id: Option<&str>,
    invite_only: bool,
) -> Result<Meeting, MeetingError> {
    let id = Uuid::new_v4().to_string();
    let id = id.as_str();
    let pool = db.pool().await?;
    let meeting = retry_on_conflict(|| async move {
        sqlx::query_as::<_, Meeting>(
            "INSERT INTO meetings (id, created_by, org_id, invite_only, idle_since)
             VALUES ($1, $2, $3, $4, now())
             RETURNING id, created_by, created_at, org_id, invite_only, idle_since",
        )
        .bind(id)
        .bind(created_by)
        .bind(org_id)
        .bind(invite_only)
        .fetch_one(pool)
        .await
    })
    .await?;

    upsert_attendee(db, &meeting.id, created_by).await?;
    Ok(meeting)
}

/// Join a meeting: record `user_id` as an attendee, then -- if that makes
/// two or more -- get its room via [`rooms::assign`] (claiming one if it
/// has none) and mint a Chime attendee in it. A solo attendee (the only one
/// present either way) claims no room and gets `"connected": false` back,
/// same shape [`leave`] pushes over [`ROOM_CHANNEL`] when a room is
/// cleared -- the frontend treats both the same way.
///
/// If claiming the room turned out to mean a *different* Chime session than
/// whatever this meeting had before (fresh mint, or recovered from one
/// Chime had already dropped), whoever else is already an attendee is no
/// longer actually connected to anything live -- each gets reconnected with
/// their own fresh Chime attendee, pushed over [`ROOM_CHANNEL`], rather than
/// left thinking they're still in a room that's gone.
pub async fn join(
    db: &Db,
    chime: &ChimeState,
    messenger: &Messenger,
    meeting_id: &str,
    user_id: &str,
) -> Result<Value, MeetingError> {
    join_inner(db, chime, messenger, meeting_id, user_id, false).await
}

/// Force `target_user_id` straight into `meeting_id`, on `caller`'s say-so
/// alone -- no invite, no accept. Only the meeting's creator may (this is
/// "start a meeting with someone", not "drag anyone into any meeting");
/// [`MeetingError::Forbidden`] otherwise. Pushed to the target over
/// [`FORCE_CHANNEL`] rather than returned, since nothing the target did
/// triggered this -- the frontend answers by connecting immediately, muted
/// and with the camera off.
pub async fn force_join(
    db: &Db,
    chime: &ChimeState,
    messenger: &Messenger,
    meeting_id: &str,
    caller: &str,
    target_user_id: &str,
) -> Result<(), MeetingError> {
    let meeting = find(db, meeting_id).await?.ok_or(MeetingError::NotFound)?;
    if meeting.created_by != caller {
        return Err(MeetingError::Forbidden);
    }

    let response = join_inner(db, chime, messenger, meeting_id, target_user_id, true).await?;
    messenger
        .send(FORCE_CHANNEL, &[target_user_id.to_string()], response)
        .await;
    Ok(())
}

/// Shared body of [`join`] and [`force_join`]: record `user_id` as an
/// attendee, and -- once that makes two or more -- claim the meeting's room
/// and mint a Chime attendee in it, reconnecting anyone else already in it
/// whose room turned out to be a fresh one. `bypass_invite_check` is what
/// tells the two callers apart: [`force_join`] has already checked its own
/// permission (the caller is the meeting's creator), so `join`'s
/// invite-only gate does not apply on top of it.
async fn join_inner(
    db: &Db,
    chime: &ChimeState,
    messenger: &Messenger,
    meeting_id: &str,
    user_id: &str,
    bypass_invite_check: bool,
) -> Result<Value, MeetingError> {
    let meeting = find(db, meeting_id).await?.ok_or(MeetingError::NotFound)?;
    if meeting.idle_expired() {
        // Nobody has swept this one yet -- do it now rather than leave it
        // for the next person who lists or tries to join it. See the
        // module doc: there is no background task for this.
        if let Err(err) = terminate(db, messenger, &meeting).await {
            tracing::warn!("meetings: could not terminate idle meeting {meeting_id}: {err:?}");
        }
        return Err(MeetingError::NotFound);
    }
    if !bypass_invite_check
        && meeting.invite_only
        && user_id != meeting.created_by
        && !meeting_invites::has_pending_invite(db, meeting_id, user_id).await?
    {
        return Err(MeetingError::Forbidden);
    }

    upsert_attendee(db, meeting_id, user_id).await?;
    let count = attendee_count(db, meeting_id).await?;

    if let Some(org_id) = &meeting.org_id {
        broadcast_list(messenger, db, org_id).await;
    }

    if count < 2 {
        set_idle_since(db, meeting_id, Some(Utc::now())).await?;
        return Ok(json!({ "meeting_id": meeting_id, "connected": false }));
    }
    set_idle_since(db, meeting_id, None).await?;

    let (room_id, session, fresh) = rooms::assign(db, chime, meeting_id).await?;

    if fresh {
        for other in attendee_ids(db, meeting_id).await? {
            if other == user_id {
                continue;
            }
            match chime.create_attendee(&room_id, &chime::external_user_id(&other)).await {
                Ok(attendee) => {
                    messenger
                        .send(
                            ROOM_CHANNEL,
                            &[other],
                            json!({
                                "meeting_id": meeting_id,
                                "connected": true,
                                "Meeting": chime::meeting_json(&session),
                                "Attendee": chime::attendee_json(&attendee),
                            }),
                        )
                        .await;
                }
                Err(err) => tracing::warn!(
                    "meetings: could not reconnect {other} to the new room for {meeting_id}: {err:?}"
                ),
            }
        }
    }

    let attendee = chime
        .create_attendee(&room_id, &chime::external_user_id(user_id))
        .await?;

    Ok(json!({
        "meeting_id": meeting_id,
        "connected": true,
        "Meeting": chime::meeting_json(&session),
        "Attendee": chime::attendee_json(&attendee),
    }))
}

/// Leave a meeting. Down to zero attendees terminates it outright (see
/// [`terminate`]); down to exactly one clears its room back to the pool --
/// proactively, rather than waiting on Chime to notice -- and starts its
/// idle clock, telling whoever's left over [`ROOM_CHANNEL`] that they're
/// disconnected from Chime without having left the meeting itself. Two or
/// more remaining attendees means nothing else needs to change.
pub async fn leave(
    db: &Db,
    messenger: &Messenger,
    meeting_id: &str,
    user_id: &str,
) -> Result<(), MeetingError> {
    let meeting = find(db, meeting_id).await?.ok_or(MeetingError::NotFound)?;
    remove_attendee(db, meeting_id, user_id).await?;
    let count = attendee_count(db, meeting_id).await?;

    if count == 0 {
        // `terminate` broadcasts the org's list itself -- nothing further
        // to do here.
        terminate(db, messenger, &meeting).await?;
        return Ok(());
    }

    if count == 1 {
        rooms::release(db, meeting_id).await?;
        set_idle_since(db, meeting_id, Some(Utc::now())).await?;
        for remaining in attendee_ids(db, meeting_id).await? {
            messenger
                .send(
                    ROOM_CHANNEL,
                    &[remaining],
                    json!({ "meeting_id": meeting_id, "connected": false }),
                )
                .await;
        }
    }

    if let Some(org_id) = &meeting.org_id {
        broadcast_list(messenger, db, org_id).await;
    }
    Ok(())
}

/// Wire a hard disconnect to [`leave`] -- see the module doc. Shares
/// [`web_messenger::CONNECTION`] with `core::rbac`'s own subscriber (that
/// channel now runs every handler registered on it, not just one, see
/// [`Messenger::on_message`](web_messenger::Messenger::on_message)); meant
/// to be called once, at startup, same as `rbac::register` and
/// `rooms::register`.
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(web_messenger::CONNECTION, move |messenger, user_id, message| {
        let db = db.clone();
        async move {
            if message.as_str() != Some(web_messenger::DISCONNECTED) {
                return;
            }
            let meeting_ids = match attendee_meeting_ids(&db, &user_id).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!(
                        "meetings: could not list meetings for disconnected user {user_id}: {err:?}"
                    );
                    return;
                }
            };
            for meeting_id in meeting_ids {
                if let Err(err) = leave(&db, &messenger, &meeting_id, &user_id).await {
                    tracing::warn!(
                        "meetings: could not leave {meeting_id} for disconnected user {user_id}: {err:?}"
                    );
                }
            }
        }
    });
}

/// Tear a meeting down for good: tell whatever attendees are still on
/// record it ended, release its room back to the pool rather than destroy
/// it outright (a stale room is [`rooms::assign`]'s problem to notice
/// lazily, the next time someone actually tries to use it -- not this
/// function's to clean up now), and delete its rows -- children before the
/// `meetings` row itself, DSQL has no cascading deletes. Called by
/// [`leave`] (the last attendee just left) and by [`join`]/[`sweep_idle`]
/// (found the meeting already idle past [`IDLE_AFTER_SECONDS`] and is
/// terminating it before doing anything else).
async fn terminate(db: &Db, messenger: &Messenger, meeting: &Meeting) -> Result<(), MeetingError> {
    for remaining in attendee_ids(db, &meeting.id).await? {
        messenger
            .send(
                ROOM_CHANNEL,
                &[remaining],
                json!({ "meeting_id": meeting.id, "ended": true }),
            )
            .await;
    }

    rooms::release(db, &meeting.id).await?;
    delete_attendees(db, &meeting.id).await?;
    meeting_invites::drop_invites_for_meeting(db, messenger, &meeting.id).await?;
    delete_meeting_row(db, &meeting.id).await?;

    if let Some(org_id) = &meeting.org_id {
        broadcast_list(messenger, db, org_id).await;
    }
    Ok(())
}

/// Terminate every meeting in `org_id` idle past [`IDLE_AFTER_SECONDS`] --
/// called lazily, by [`list_route`], rather than on any kind of timer; see
/// the module doc. Logs and moves on rather than letting one meeting's
/// failure block the rest.
async fn sweep_idle(db: &Db, messenger: &Messenger, org_id: &str) {
    let expired = match idle_expired_in_org(db, org_id).await {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!("meetings: could not list idle meetings in org {org_id}: {err:?}");
            return;
        }
    };
    for meeting in expired {
        tracing::info!(
            "meeting {} idle past {IDLE_AFTER_SECONDS}s, terminating",
            meeting.id
        );
        if let Err(err) = terminate(db, messenger, &meeting).await {
            tracing::warn!(
                "meetings: could not terminate idle meeting {}: {err:?}",
                meeting.id
            );
        }
    }
}

#[derive(sqlx::FromRow)]
struct MeetingAttendeeRow {
    id: String,
    created_by: String,
    created_at: DateTime<Utc>,
    org_id: Option<String>,
    invite_only: bool,
    idle_since: Option<DateTime<Utc>>,
    attendee_id: Option<String>,
    pending_invites: bool,
}

/// Every meeting in `org_id`, newest first, each with who's currently in it
/// -- any member sees and can rejoin any of them, not just their own.
/// Excludes anything already idle past [`IDLE_AFTER_SECONDS`] as a
/// defensive filter -- [`list_route`] should already have swept those away
/// before calling this, but nobody should be able to join one in the
/// meantime regardless, so there is no reason to show it either.
pub async fn list(db: &Db, org_id: &str) -> Result<Vec<MeetingWithAttendees>, MeetingError> {
    let pool = db.pool().await?;
    let rows: Vec<MeetingAttendeeRow> = sqlx::query_as(
        "SELECT m.id, m.created_by, m.created_at, m.org_id, m.invite_only, m.idle_since,
                a.user_id AS attendee_id,
                EXISTS(SELECT 1 FROM meeting_invites mi WHERE mi.meeting_id = m.id) AS pending_invites
         FROM meetings m
         LEFT JOIN meeting_attendees a ON a.meeting_id = m.id
         WHERE m.id IN (
             SELECT id FROM meetings
             WHERE org_id = $1
               AND (idle_since IS NULL OR idle_since > now() - ($2 * interval '1 second'))
             ORDER BY created_at DESC
             LIMIT 100
         )
         ORDER BY m.created_at DESC, m.id",
    )
    .bind(org_id)
    .bind(IDLE_AFTER_SECONDS)
    .fetch_all(pool)
    .await?;

    let mut result: Vec<MeetingWithAttendees> = Vec::new();
    for row in rows {
        match result.last_mut() {
            Some(last) if last.meeting.id == row.id => {
                if let Some(attendee_id) = row.attendee_id {
                    last.attendees.push(attendee_id);
                }
            }
            _ => result.push(MeetingWithAttendees {
                meeting: Meeting {
                    id: row.id,
                    created_by: row.created_by,
                    created_at: row.created_at,
                    org_id: row.org_id,
                    invite_only: row.invite_only,
                    idle_since: row.idle_since,
                },
                attendees: row.attendee_id.into_iter().collect(),
                pending_invites: row.pending_invites,
            }),
        }
    }
    Ok(result)
}

/// Push `org_id`'s current meeting list to every member of it who is
/// currently online, on [`LIST_CHANNEL`] -- best effort, logging rather
/// than failing, since callers have nothing left to hand a `MeetingError`
/// back to by the time a meeting was already created or torn down.
pub(crate) async fn broadcast_list(messenger: &Messenger, db: &Db, org_id: &str) {
    let meetings = match list(db, org_id).await {
        Ok(meetings) => meetings,
        Err(err) => {
            tracing::warn!("meetings: could not list org {org_id} for broadcast: {err:?}");
            return;
        }
    };
    let online_ids = match rbac::list_members(db, org_id).await {
        Ok(members) => members
            .into_iter()
            .filter(|member| member.online)
            .map(|member| member.user_id)
            .collect::<Vec<_>>(),
        Err(err) => {
            tracing::warn!("meetings: could not list members of org {org_id}: {err:?}");
            return;
        }
    };
    messenger
        .send(
            LIST_CHANNEL,
            &online_ids,
            json!({ "org_id": org_id, "meetings": meetings }),
        )
        .await;
}

#[derive(Debug, Deserialize)]
struct CreateMeetingBody {
    org_id: String,
    #[serde(default)]
    invite_only: bool,
}

async fn create_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateMeetingBody>,
) -> Result<Json<Value>, MeetingError> {
    rbac::require_member(&db, &body.org_id, &claims.sub).await?;
    let meeting = create(&db, &claims.sub, Some(&body.org_id), body.invite_only).await?;
    broadcast_list(&messenger, &db, &body.org_id).await;
    Ok(Json(json!({ "meeting": meeting })))
}

#[derive(Debug, Deserialize)]
struct ListMeetingsParams {
    org_id: String,
}

async fn list_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<ListMeetingsParams>,
) -> Result<Json<Value>, MeetingError> {
    rbac::require_member(&db, &params.org_id, &claims.sub).await?;
    // Lazily terminate anything idle past its window before answering --
    // see the module doc. This is the other half of that, alongside `join`
    // catching whichever meeting it was actually asked for.
    sweep_idle(&db, &messenger, &params.org_id).await;
    let meetings = list(&db, &params.org_id).await?;
    Ok(Json(json!({ "meetings": meetings })))
}

async fn join_route(
    Extension(db): Extension<Db>,
    Extension(chime): Extension<ChimeState>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    let response = join(&db, &chime, &messenger, &meeting_id, &claims.sub).await?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct ForceJoinBody {
    user_id: String,
}

/// `POST /meeting/{meeting_id}/force` -- force `user_id` straight into the
/// meeting, no invite or accept. Only the meeting's creator may; see
/// [`force_join`].
async fn force_join_route(
    Extension(db): Extension<Db>,
    Extension(chime): Extension<ChimeState>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(meeting_id): Path<String>,
    Json(body): Json<ForceJoinBody>,
) -> Result<Json<Value>, MeetingError> {
    force_join(
        &db,
        &chime,
        &messenger,
        &meeting_id,
        &claims.sub,
        &body.user_id,
    )
    .await?;
    Ok(Json(
        json!({ "meeting_id": meeting_id, "forced": body.user_id }),
    ))
}

async fn leave_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    leave(&db, &messenger, &meeting_id, &claims.sub).await?;
    Ok(Json(
        json!({ "meeting_id": meeting_id, "left_by": claims.sub }),
    ))
}

#[derive(Debug, Deserialize)]
struct CreateInviteBody {
    invited_user_id: String,
}

/// `POST /meeting/{meeting_id}/invites` -- invite someone to a meeting. Any
/// current attendee may, not just whoever created it -- the meeting keeps
/// going after its creator leaves (see `leave`), and whoever's left in it
/// should still be able to bring someone else in rather than being stuck
/// once the original creator is gone.
async fn create_invite_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(meeting_id): Path<String>,
    Json(body): Json<CreateInviteBody>,
) -> Result<Json<Value>, MeetingError> {
    find(&db, &meeting_id)
        .await?
        .ok_or(MeetingError::NotFound)?;
    if !attendee_ids(&db, &meeting_id).await?.contains(&claims.sub) {
        return Err(MeetingError::Forbidden);
    }
    let invite = meeting_invites::create_invite(
        &db,
        &messenger,
        &meeting_id,
        &body.invited_user_id,
        &claims.sub,
    )
    .await?;
    Ok(Json(json!({ "invite": invite })))
}

/// `DELETE /meeting/{meeting_id}/invites/{invite_id}` -- revoke. Any current
/// attendee may, same as sending one -- see `create_invite_route`.
async fn revoke_invite_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path((meeting_id, invite_id)): Path<(String, String)>,
) -> Result<Json<Value>, MeetingError> {
    find(&db, &meeting_id)
        .await?
        .ok_or(MeetingError::NotFound)?;
    if !attendee_ids(&db, &meeting_id).await?.contains(&claims.sub) {
        return Err(MeetingError::Forbidden);
    }
    meeting_invites::revoke_invite(&db, &messenger, &meeting_id, &invite_id).await?;
    Ok(Json(json!({ "revoked": invite_id })))
}

/// `GET /meeting/invites` -- every pending invite addressed to the caller,
/// across every org.
async fn list_invites_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, MeetingError> {
    let invites = meeting_invites::invites_for_user(&db, &claims.sub).await?;
    Ok(Json(json!({ "invites": invites })))
}

/// `POST /meeting/invites/{invite_id}/accept` -- join the invite's meeting
/// and drop it. No further check beyond the invite being addressed to the
/// caller.
async fn accept_invite_route(
    Extension(db): Extension<Db>,
    Extension(chime): Extension<ChimeState>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    let response =
        meeting_invites::accept_invite(&db, &chime, &messenger, &invite_id, &claims.sub).await?;
    Ok(Json(response))
}

/// `POST /meeting/invites/{invite_id}/decline` -- the invited side's "no
/// thanks".
async fn decline_invite_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    meeting_invites::decline_invite(&db, &messenger, &invite_id, &claims.sub).await?;
    Ok(Json(json!({ "declined": invite_id })))
}

/// `/meeting/*` -- create/join/leave/list, invites, plus chat and room
/// status nested in from the sibling modules.
pub fn routes() -> Router {
    Router::new()
        .route("/", get(list_route).post(create_route))
        .route("/invites", get(list_invites_route))
        .route("/invites/{invite_id}/accept", post(accept_invite_route))
        .route("/invites/{invite_id}/decline", post(decline_invite_route))
        .route("/{meeting_id}/join", post(join_route))
        .route("/{meeting_id}/force", post(force_join_route))
        .route("/{meeting_id}/leave", post(leave_route))
        .route("/{meeting_id}/invites", post(create_invite_route))
        .route(
            "/{meeting_id}/invites/{invite_id}",
            delete(revoke_invite_route),
        )
}
