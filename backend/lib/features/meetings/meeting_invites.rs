//! Meeting invitations -- gated on the meeting's creator, targeting a
//! specific already-known org member rather than an arbitrary email:
//! unlike [`crate::core::rbac::org_invites`] there is no anonymous
//! magic-link path here, since the frontend only ever offers this from a
//! live roster of people already online. An invite is never left around
//! once it's resolved one way or the other -- accepting joins the meeting
//! and drops the row, declining does too, and so does the meeting ending --
//! so an invite's own existence *is* its pending state; there is no
//! separate status to track.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::{attendee_count, broadcast_list, join, set_idle_since, MeetingError};
use crate::core::chime::ChimeState;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::{retry_on_conflict, Db, DbError};

/// Pushes `{"invites": [MeetingInvite, ...]}` -- every pending invite for
/// one user -- to that user alone, whenever the list changes: a new
/// invite, or one dropped by acceptance, decline, revocation, or its
/// meeting ending.
pub const CHANNEL: web_messenger::Channel = "meeting_invites";

#[derive(Debug, Serialize)]
pub struct MeetingInvite {
    pub id: String,
    pub meeting_id: String,
    pub org_id: Option<String>,
    pub invited_user_id: String,
    pub invited_by: String,
    /// Name, else email, else the id itself -- see `From<InviteRow>`.
    pub invited_by_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct InviteRow {
    id: String,
    meeting_id: String,
    org_id: Option<String>,
    invited_user_id: String,
    invited_by: String,
    invited_by_name: Option<String>,
    invited_by_email: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<InviteRow> for MeetingInvite {
    fn from(row: InviteRow) -> Self {
        let invited_by_name = row
            .invited_by_name
            .filter(|name| !name.trim().is_empty())
            .or(row.invited_by_email)
            .unwrap_or_else(|| row.invited_by.clone());
        Self {
            id: row.id,
            meeting_id: row.meeting_id,
            org_id: row.org_id,
            invited_user_id: row.invited_user_id,
            invited_by: row.invited_by,
            invited_by_name,
            created_at: row.created_at,
        }
    }
}

async fn get_invite(db: &Db, invite_id: &str) -> Result<Option<MeetingInvite>, DbError> {
    let pool = db.pool().await?;
    let row: Option<InviteRow> = sqlx::query_as(
        "SELECT mi.id, mi.meeting_id, m.org_id, mi.invited_user_id, mi.invited_by,
                u.name AS invited_by_name, u.email AS invited_by_email, mi.created_at
         FROM meeting_invites mi
         JOIN meetings m ON m.id = mi.meeting_id
         JOIN users u ON u.id = mi.invited_by
         WHERE mi.id = $1",
    )
    .bind(invite_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(MeetingInvite::from))
}

/// Every pending invite for `user_id`, across every org and meeting --
/// newest first.
pub async fn invites_for_user(db: &Db, user_id: &str) -> Result<Vec<MeetingInvite>, DbError> {
    let pool = db.pool().await?;
    let rows: Vec<InviteRow> = sqlx::query_as(
        "SELECT mi.id, mi.meeting_id, m.org_id, mi.invited_user_id, mi.invited_by,
                u.name AS invited_by_name, u.email AS invited_by_email, mi.created_at
         FROM meeting_invites mi
         JOIN meetings m ON m.id = mi.meeting_id
         JOIN users u ON u.id = mi.invited_by
         WHERE mi.invited_user_id = $1
         ORDER BY mi.created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(MeetingInvite::from).collect())
}

/// True if `user_id` currently holds a pending invite to `meeting_id` --
/// [`super::join`]'s other way in for an invite-only meeting, besides being
/// its creator.
pub(crate) async fn has_pending_invite(
    db: &Db,
    meeting_id: &str,
    user_id: &str,
) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let row: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM meeting_invites WHERE meeting_id = $1 AND invited_user_id = $2",
    )
    .bind(meeting_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// True if `meeting_id` has any pending invite at all, regardless of who
/// it's addressed to -- unlike [`has_pending_invite`], which checks one
/// particular invitee. Used to decide whether a solo meeting's idle clock
/// may resume once one invite is dropped: only if nothing else is still
/// pending on it.
async fn has_any_pending_invite(db: &Db, meeting_id: &str) -> Result<bool, DbError> {
    let pool = db.pool().await?;
    let row: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM meeting_invites WHERE meeting_id = $1 LIMIT 1")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// A solo meeting (fewer than two attendees) pauses its idle clock while
/// `meeting_id` has a pending invite -- see the module doc on
/// [`super::MeetingWithAttendees::pending_invites`]: nobody should get
/// swept out from under an invite they just sent, or land back in `join`'s
/// [`super::Meeting::idle_expired`] gate and find it already gone. A
/// meeting with two or more attendees already keeps `idle_since` cleared on
/// its own (see `join`), so this only ever touches the solo case.
async fn pause_idle_if_solo(db: &Db, meeting_id: &str) -> Result<(), DbError> {
    if attendee_count(db, meeting_id).await? < 2 {
        set_idle_since(db, meeting_id, None).await?;
    }
    Ok(())
}

/// The other half of [`pause_idle_if_solo`]: once an invite is dropped
/// (revoked, declined, or its meeting-invite row otherwise removed) with
/// nothing else pending on a still-solo meeting, resume its idle clock from
/// now rather than leave it paused forever.
async fn resume_idle_if_unblocked(db: &Db, meeting_id: &str) -> Result<(), DbError> {
    if attendee_count(db, meeting_id).await? < 2 && !has_any_pending_invite(db, meeting_id).await? {
        set_idle_since(db, meeting_id, Some(Utc::now())).await?;
    }
    Ok(())
}

async fn delete_invite(db: &Db, invite_id: &str) -> Result<(), DbError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM meeting_invites WHERE id = $1")
            .bind(invite_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
}

/// Push `user_id`'s current invite list to them -- call after anything
/// changes it. Best effort, logging rather than failing: by the time an
/// invite has been created or dropped there is nothing left to hand a
/// `MeetingError` back to.
pub async fn broadcast_invites(messenger: &Messenger, db: &Db, user_id: &str) {
    match invites_for_user(db, user_id).await {
        Ok(invites) => {
            messenger
                .send(
                    CHANNEL,
                    &[user_id.to_string()],
                    json!({ "invites": invites }),
                )
                .await;
        }
        Err(err) => {
            tracing::warn!("meeting_invites: could not list invites for {user_id}: {err:?}")
        }
    }
}

/// Create a pending invite, reusing one already pending for the same
/// (meeting, invited user) rather than piling up duplicates, and push the
/// invited user their fresh list.
pub async fn create_invite(
    db: &Db,
    messenger: &Messenger,
    meeting_id: &str,
    invited_user_id: &str,
    invited_by: &str,
) -> Result<MeetingInvite, MeetingError> {
    let pool = db.pool().await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM meeting_invites WHERE meeting_id = $1 AND invited_user_id = $2",
    )
    .bind(meeting_id)
    .bind(invited_user_id)
    .fetch_optional(pool)
    .await?;

    let id = match existing {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4().to_string();
            retry_on_conflict(|| {
                let id = id.clone();
                async move {
                    sqlx::query(
                        "INSERT INTO meeting_invites (id, meeting_id, invited_user_id, invited_by)
                         VALUES ($1, $2, $3, $4)",
                    )
                    .bind(&id)
                    .bind(meeting_id)
                    .bind(invited_user_id)
                    .bind(invited_by)
                    .execute(pool)
                    .await
                    .map(|_| ())
                }
            })
            .await?;
            id
        }
    };

    pause_idle_if_solo(db, meeting_id).await?;
    let invite = get_invite(db, &id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    broadcast_invites(messenger, db, invited_user_id).await;
    if let Some(org_id) = &invite.org_id {
        broadcast_list(messenger, db, org_id).await;
    }
    Ok(invite)
}

/// Revoke a still-pending invite. `meeting_id` is checked so the creator of
/// one meeting can't revoke another meeting's invite by id.
pub async fn revoke_invite(
    db: &Db,
    messenger: &Messenger,
    meeting_id: &str,
    invite_id: &str,
) -> Result<(), MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.meeting_id != meeting_id {
        return Err(MeetingError::InviteNotFound);
    }
    delete_invite(db, invite_id).await?;
    resume_idle_if_unblocked(db, meeting_id).await?;
    broadcast_invites(messenger, db, &invite.invited_user_id).await;
    if let Some(org_id) = &invite.org_id {
        broadcast_list(messenger, db, org_id).await;
    }
    Ok(())
}

/// Decline a pending invite -- the invited side's "no thanks". `user_id`
/// must be the one it was sent to.
pub async fn decline_invite(
    db: &Db,
    messenger: &Messenger,
    invite_id: &str,
    user_id: &str,
) -> Result<(), MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.invited_user_id != user_id {
        return Err(MeetingError::Forbidden);
    }
    delete_invite(db, invite_id).await?;
    resume_idle_if_unblocked(db, &invite.meeting_id).await?;
    broadcast_invites(messenger, db, user_id).await;
    if let Some(org_id) = &invite.org_id {
        broadcast_list(messenger, db, org_id).await;
    }
    Ok(())
}

/// Accept a pending invite: join its meeting -- the one path that can join
/// an invite-only meeting as anyone but its creator, see
/// [`has_pending_invite`] -- then drop the invite now that it's been
/// redeemed. The invite is looked up and checked, but not yet deleted,
/// before `join` runs: `join`'s own invite-only gate needs it to still
/// exist to let this caller in.
pub async fn accept_invite(
    db: &Db,
    chime: &ChimeState,
    messenger: &Messenger,
    invite_id: &str,
    user_id: &str,
) -> Result<Value, MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.invited_user_id != user_id {
        return Err(MeetingError::Forbidden);
    }
    let response = join(db, chime, messenger, &invite.meeting_id, user_id).await?;
    delete_invite(db, invite_id).await?;
    broadcast_invites(messenger, db, user_id).await;
    Ok(response)
}

/// Delete every pending invite for `meeting_id` and tell each invited user
/// their list just changed -- called once a meeting is gone (see
/// [`super::terminate`]), so nothing keeps inviting anyone to a meeting
/// that no longer exists.
pub(crate) async fn drop_invites_for_meeting(
    db: &Db,
    messenger: &Messenger,
    meeting_id: &str,
) -> Result<(), DbError> {
    let pool = db.pool().await?;
    let invited: Vec<String> =
        sqlx::query_scalar("SELECT invited_user_id FROM meeting_invites WHERE meeting_id = $1")
            .bind(meeting_id)
            .fetch_all(pool)
            .await?;

    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM meeting_invites WHERE meeting_id = $1")
            .bind(meeting_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;

    for user_id in invited {
        broadcast_invites(messenger, db, &user_id).await;
    }
    Ok(())
}
