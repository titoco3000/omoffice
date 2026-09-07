//! Meeting invitations — `rbac::invites`' twin, gated on the meeting's
//! creator rather than a role, with no roles of its own to carry.

use axum::{
    extract::{Extension, Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::{Meeting, MeetingError};
use crate::auth::{AuthState, Claims};
use crate::core::apigw_management::ApiGatewayManagement;
use crate::core::db::{retry_on_conflict, Db};
use crate::core::messenger::Messenger;
use crate::users;

/// One row of `meeting_invites`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MeetingInvite {
    pub id: String,
    pub code: String,
    pub email: String,
    pub invited_by: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

async fn get_invite(db: &Db, invite_id: &str) -> Result<Option<MeetingInvite>, MeetingError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, MeetingInvite>(
        "SELECT id, code, email, invited_by, status, created_at, accepted_at
         FROM meeting_invites WHERE id = $1",
    )
    .bind(invite_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Create a pending invite, reusing an existing pending `(code, email)` row
/// rather than piling up duplicates.
pub async fn create_invite(
    db: &Db,
    code: &str,
    email: &str,
    invited_by: &str,
) -> Result<MeetingInvite, MeetingError> {
    let email = email.trim().to_lowercase();
    let pool = db.pool().await?;

    let existing_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM meeting_invites WHERE code = $1 AND email = $2 AND status = 'pending'",
    )
    .bind(code)
    .bind(&email)
    .fetch_optional(pool)
    .await?;

    let id = match existing_id {
        Some(id) => {
            retry_on_conflict(|| {
                let id = id.clone();
                async move {
                    sqlx::query("UPDATE meeting_invites SET created_at = now() WHERE id = $1")
                        .bind(&id)
                        .execute(pool)
                        .await
                        .map(|_| ())
                }
            })
            .await?;
            id
        }
        None => {
            let id = Uuid::new_v4().to_string();
            retry_on_conflict(|| {
                let id = id.clone();
                let email = email.clone();
                async move {
                    sqlx::query(
                        "INSERT INTO meeting_invites (id, code, email, invited_by)
                         VALUES ($1, $2, $3, $4)",
                    )
                    .bind(&id)
                    .bind(code)
                    .bind(&email)
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

    get_invite(db, &id)
        .await?
        .ok_or(MeetingError::InviteNotFound)
}

/// Every meeting invite still pending for `email` — the meeting half of
/// `GET /private/invites`.
pub async fn pending_invites_for_email(
    db: &Db,
    email: &str,
) -> Result<Vec<MeetingInvite>, MeetingError> {
    let pool = db.pool().await?;
    let rows = sqlx::query_as::<_, MeetingInvite>(
        "SELECT id, code, email, invited_by, status, created_at, accepted_at
         FROM meeting_invites WHERE email = $1 AND status = 'pending' ORDER BY created_at DESC",
    )
    .bind(email.trim().to_lowercase())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Accept a pending invite: mark it `accepted`. No membership row to write
/// — the frontend still calls `attendees` itself with the token this hands back.
pub async fn accept_invite(db: &Db, invite_id: &str) -> Result<MeetingInvite, MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.status != "pending" {
        return Err(MeetingError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query(
                "UPDATE meeting_invites SET status = 'accepted', accepted_at = now() WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;
    Ok(invite)
}

/// Revoke a pending invite. `code` is checked so the creator of one meeting
/// can't revoke another meeting's invite by id.
pub async fn revoke_invite(db: &Db, code: &str, invite_id: &str) -> Result<(), MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.code != code {
        return Err(MeetingError::InviteNotFound);
    }
    if invite.status != "pending" {
        return Err(MeetingError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query("UPDATE meeting_invites SET status = 'revoked' WHERE id = $1")
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Decline a pending invite — the invited side's "no thanks". `email` must
/// match the invite's.
pub async fn decline_invite(db: &Db, invite_id: &str, email: &str) -> Result<(), MeetingError> {
    let invite = get_invite(db, invite_id)
        .await?
        .ok_or(MeetingError::InviteNotFound)?;
    if invite.email != email.trim().to_lowercase() {
        return Err(MeetingError::Forbidden);
    }
    if invite.status != "pending" {
        return Err(MeetingError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query("UPDATE meeting_invites SET status = 'declined' WHERE id = $1")
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Email the invite and, if that address has a live session open, push a
/// toast to it now. Best effort on both.
async fn notify_invite(
    db: &Db,
    messenger: &Messenger,
    management: &ApiGatewayManagement,
    invite: &MeetingInvite,
) {
    let existing_user = users::exists(db, &invite.email).await.unwrap_or(false);
    let accept_url = format!(
        "{}/?invite=meeting:{}",
        crate::core::frontend_base_url(),
        invite.id
    );
    let email_body = json!({
        "template": if existing_user { "meeting_invite_existing_user" } else { "meeting_invite_new_user" },
        "code": invite.code,
        "accept_url": accept_url,
    });
    messenger.email().send(&invite.email, &email_body).await;

    let toast = json!({
        "type": "invite",
        "kind": "meeting",
        "invite_id": invite.id,
        "code": invite.code,
        "message": "You've been invited to a meeting",
    });
    messenger
        .push_toast(db, management, &invite.email, &toast)
        .await;
}

#[derive(Debug, Deserialize)]
pub struct CreateMeetingInviteBody {
    email: String,
}

/// `POST /private/meetings/{code}/invites` — invite someone to a meeting.
/// Only the meeting's creator may.
pub async fn create_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(messenger): Extension<Messenger>,
    Extension(management): Extension<ApiGatewayManagement>,
    Path(code): Path<String>,
    Json(body): Json<CreateMeetingInviteBody>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = Meeting::get(&db, &code)
        .await?
        .ok_or(MeetingError::NotFound)?;
    if meeting.created_by != claims.sub {
        return Err(MeetingError::Forbidden);
    }
    let invite = create_invite(&db, &code, &body.email, &claims.sub).await?;
    notify_invite(&db, &messenger, &management, &invite).await;
    Ok(Json(json!({ "invite": invite })))
}

/// `DELETE /private/meetings/{code}/invites/{invite_id}` — revoke. Only the
/// meeting's creator may.
pub async fn revoke_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path((code, invite_id)): Path<(String, String)>,
) -> Result<Json<Value>, MeetingError> {
    let meeting = Meeting::get(&db, &code)
        .await?
        .ok_or(MeetingError::NotFound)?;
    if meeting.created_by != claims.sub {
        return Err(MeetingError::Forbidden);
    }
    revoke_invite(&db, &code, &invite_id).await?;
    Ok(Json(json!({ "revoked": invite_id })))
}

/// `POST /invites/meetings/{invite_id}/accept` — the magic link: public,
/// same trust model as `rbac::accept_invite_route`.
pub async fn accept_invite_route(
    State(auth): State<AuthState>,
    Extension(db): Extension<Db>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    let invite = accept_invite(&db, &invite_id).await?;
    let (token, expires_at) = auth.issue(&invite.email).await?;
    Ok(Json(json!({
        "token": token,
        "token_type": "Bearer",
        "email": invite.email,
        "expires_at": expires_at,
        "code": invite.code,
    })))
}

/// `POST /private/invites/meetings/{invite_id}/decline` — the invited
/// side's "no thanks", for someone already logged in.
pub async fn decline_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, MeetingError> {
    decline_invite(&db, &invite_id, &claims.sub).await?;
    Ok(Json(json!({ "declined": invite_id })))
}
