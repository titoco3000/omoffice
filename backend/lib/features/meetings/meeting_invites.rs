//! Meeting invitations -- [`crate::core::rbac::org_invites`]'s twin, gated
//! on the meeting's creator rather than a role, and smarter about delivery:
//! a member who currently has a live socket gets a toast over it instead of
//! waiting on email.

use chrono::{DateTime, Utc};

use super::MeetingError;
use crate::core::communication::{email, web_messenger::Messenger};
use crate::core::data::db::Db;

pub const CHANNEL: &str = "meeting_invites";

#[derive(Debug)]
pub struct MeetingInvite {
    pub id: String,
    pub meeting_id: String,
    pub email: String,
    pub invited_by: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_status_change: DateTime<Utc>,
}

/// Create a pending invite, then deliver it: a toast over the socket if
/// `email` is currently connected, an email otherwise -- the one place this
/// module's delivery choice differs from `rbac::org_invites`, which always emails.
pub async fn create_invite(
    db: &Db,
    messenger: &Messenger,
    meeting_id: &str,
    email_addr: &str,
    invited_by: &str,
) -> Result<MeetingInvite, MeetingError> {
    let invite = insert_invite(db, meeting_id, email_addr, invited_by).await?;

    if messenger.is_connected(email_addr).await {
        messenger
            .send(
                CHANNEL,
                &[email_addr.to_string()],
                serde_json::json!({ "invite_id": invite.id, "meeting_id": meeting_id }),
            )
            .await;
    } else {
        let magic_link = format!(
            "{}/?invite=meeting:{}",
            crate::core::frontend_base_url(),
            invite.id
        );
        email::send_email(
            "noreply@omoffice.example",
            email_addr,
            "You've been invited to a meeting",
            &magic_link,
        );
    }

    Ok(invite)
}

async fn insert_invite(
    db: &Db,
    meeting_id: &str,
    email_addr: &str,
    invited_by: &str,
) -> Result<MeetingInvite, MeetingError> {
    let _ = (db, meeting_id, email_addr, invited_by);
    todo!(
        "insert/update a pending meeting_invites row, reusing one for the same (meeting_id, email)"
    )
}

pub async fn accept_invite(db: &Db, invite_id: &str) -> Result<MeetingInvite, MeetingError> {
    let _ = (db, invite_id);
    todo!()
}

pub async fn decline_invite(db: &Db, invite_id: &str) -> Result<(), MeetingError> {
    let _ = (db, invite_id);
    todo!()
}

/// `meeting_id` is checked so the creator of one meeting can't revoke
/// another meeting's invite by id.
pub async fn revoke_invite(db: &Db, meeting_id: &str, invite_id: &str) -> Result<(), MeetingError> {
    let _ = (db, meeting_id, invite_id);
    todo!()
}
