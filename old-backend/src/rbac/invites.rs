//! Org invitations: a pending row that only becomes membership on accept.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::orgs::add_member;
use super::{get_org, normalize_email, org_exists, AllowedRoles, Org, RbacError};
use crate::core::db::{retry_on_conflict, Db};
use crate::core::messenger::{Messenger, Via};
use crate::users;

/// The channel invite notifications ride on, org and meeting alike;
/// `frontend/src/hooks/useNotifications.js` subscribes to it.
pub const CHANNEL: &str = "invites";

fn split_roles(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(',').map(str::to_string).collect()
    }
}

fn join_roles(roles: &[String]) -> String {
    roles.join(",")
}

#[derive(sqlx::FromRow)]
struct OrgInviteRow {
    id: String,
    org_id: String,
    email: String,
    roles: String,
    invited_by: String,
    status: String,
    created_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
}

/// A pending (or already-settled) invitation into an org, as the API sees it
/// — `roles` split back into a list, same shape [`add_member`] takes.
#[derive(Debug, Serialize)]
pub struct OrgInvite {
    pub id: String,
    pub org_id: String,
    pub email: String,
    pub roles: Vec<String>,
    pub invited_by: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

impl From<OrgInviteRow> for OrgInvite {
    fn from(row: OrgInviteRow) -> Self {
        Self {
            id: row.id,
            org_id: row.org_id,
            email: row.email,
            roles: split_roles(&row.roles),
            invited_by: row.invited_by,
            status: row.status,
            created_at: row.created_at,
            accepted_at: row.accepted_at,
        }
    }
}

/// One entry in `GET /private/invites` — the org's name folded in, since
/// that route is keyed by the invited email, not the org.
#[derive(sqlx::FromRow)]
struct PendingOrgInviteRow {
    id: String,
    org_id: String,
    org_name: String,
    roles: String,
    invited_by: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PendingOrgInvite {
    pub id: String,
    pub org_id: String,
    pub org_name: String,
    pub roles: Vec<String>,
    pub invited_by: String,
    pub created_at: DateTime<Utc>,
}

impl From<PendingOrgInviteRow> for PendingOrgInvite {
    fn from(row: PendingOrgInviteRow) -> Self {
        Self {
            id: row.id,
            org_id: row.org_id,
            org_name: row.org_name,
            roles: split_roles(&row.roles),
            invited_by: row.invited_by,
            created_at: row.created_at,
        }
    }
}

pub(super) async fn get_org_invite(
    db: &Db,
    invite_id: &str,
) -> Result<Option<OrgInvite>, RbacError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, OrgInviteRow>(
        "SELECT id, org_id, email, roles, invited_by, status, created_at, accepted_at
         FROM org_invites WHERE id = $1",
    )
    .bind(invite_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(OrgInvite::from))
}

/// Create a pending invite, reusing an existing pending `(org_id, email)`
/// row. Does not touch `org_roles`; only [`accept_invite`] does.
pub async fn create_invite(
    db: &Db,
    org_id: &str,
    email: &str,
    roles: &[String],
    invited_by: &str,
) -> Result<OrgInvite, RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let email = normalize_email(email);
    let roles_joined = join_roles(roles);
    let pool = db.pool().await?;

    let existing_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM org_invites WHERE org_id = $1 AND email = $2 AND status = 'pending'",
    )
    .bind(org_id)
    .bind(&email)
    .fetch_optional(pool)
    .await?;

    let id = match existing_id {
        Some(id) => {
            retry_on_conflict(|| {
                let id = id.clone();
                let roles_joined = roles_joined.clone();
                async move {
                    sqlx::query(
                        "UPDATE org_invites SET roles = $1, created_at = now() WHERE id = $2",
                    )
                    .bind(&roles_joined)
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
                let roles_joined = roles_joined.clone();
                async move {
                    sqlx::query(
                        "INSERT INTO org_invites (id, org_id, email, roles, invited_by)
                         VALUES ($1, $2, $3, $4, $5)",
                    )
                    .bind(&id)
                    .bind(org_id)
                    .bind(&email)
                    .bind(&roles_joined)
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

    get_org_invite(db, &id)
        .await?
        .ok_or(RbacError::InviteNotFound)
}

/// The org's still-pending invites — `GET /private/orgs/{org_id}/invites`.
pub async fn list_invites(db: &Db, org_id: &str) -> Result<Vec<OrgInvite>, RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let pool = db.pool().await?;
    let rows: Vec<OrgInviteRow> = sqlx::query_as(
        "SELECT id, org_id, email, roles, invited_by, status, created_at, accepted_at
         FROM org_invites WHERE org_id = $1 AND status = 'pending' ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(OrgInvite::from).collect())
}

/// Every org invite still pending for `email` — the org half of
/// `GET /private/invites`.
pub async fn pending_invites_for_email(
    db: &Db,
    email: &str,
) -> Result<Vec<PendingOrgInvite>, RbacError> {
    let pool = db.pool().await?;
    let rows: Vec<PendingOrgInviteRow> = sqlx::query_as(
        "SELECT i.id, i.org_id, o.name AS org_name, i.roles, i.invited_by, i.created_at
         FROM org_invites i
         JOIN organizations o ON o.id = i.org_id
         WHERE i.email = $1 AND i.status = 'pending'
         ORDER BY i.created_at DESC",
    )
    .bind(normalize_email(email))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(PendingOrgInvite::from).collect())
}

/// Accept a pending invite, then [`add_member`] with its roles. Returns the
/// settled invite plus the org, so the route needs no second fetch.
pub async fn accept_invite(
    db: &Db,
    allowed: &AllowedRoles,
    invite_id: &str,
) -> Result<(OrgInvite, Org), RbacError> {
    let invite = get_org_invite(db, invite_id)
        .await?
        .ok_or(RbacError::InviteNotFound)?;
    if invite.status != "pending" {
        return Err(RbacError::InviteNotPending);
    }

    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query(
                "UPDATE org_invites SET status = 'accepted', accepted_at = now() WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;

    add_member(db, allowed, &invite.org_id, &invite.email, &invite.roles).await?;
    let org = get_org(db, &invite.org_id)
        .await?
        .ok_or(RbacError::OrgNotFound)?;
    Ok((invite, org))
}

/// Revoke a pending invite — the inviting side's cancel. `org_id` is checked
/// so a `team-management` holder in one org can't revoke another's invite.
pub async fn revoke_invite(db: &Db, org_id: &str, invite_id: &str) -> Result<(), RbacError> {
    let invite = get_org_invite(db, invite_id)
        .await?
        .ok_or(RbacError::InviteNotFound)?;
    if invite.org_id != org_id {
        return Err(RbacError::InviteNotFound);
    }
    if invite.status != "pending" {
        return Err(RbacError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query("UPDATE org_invites SET status = 'revoked' WHERE id = $1")
                .bind(&id)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Decline a pending invite. `email` must match the invite's — nothing
/// else here proves who's asking.
pub async fn decline_invite(db: &Db, invite_id: &str, email: &str) -> Result<(), RbacError> {
    let invite = get_org_invite(db, invite_id)
        .await?
        .ok_or(RbacError::InviteNotFound)?;
    if invite.email != normalize_email(email) {
        return Err(RbacError::Forbidden);
    }
    if invite.status != "pending" {
        return Err(RbacError::InviteNotPending);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let id = invite.id.clone();
        async move {
            sqlx::query("UPDATE org_invites SET status = 'declined' WHERE id = $1")
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
/// toast to it now. Best effort on both — the invite row is already
/// committed. Two calls rather than one `via: [Websocket, Email]`, because
/// the two bodies are worded differently: an email template, and a toast the
/// browser can turn into an accept button.
pub(super) async fn notify_invite(
    db: &Db,
    messenger: &Messenger,
    org_name: &str,
    invite: &OrgInvite,
) {
    let existing_user = users::exists(db, &invite.email).await.unwrap_or(false);
    let accept_url = format!(
        "{}/?invite=org:{}",
        crate::core::frontend_base_url(),
        invite.id
    );
    let to = [invite.email.clone()];

    let email_body = json!({
        "template": if existing_user { "org_invite_existing_user" } else { "org_invite_new_user" },
        "org": org_name,
        "accept_url": accept_url,
    });
    messenger
        .send(&email_body, CHANNEL, &[Via::Email], &to)
        .await;

    let toast = json!({
        "type": "invite",
        "kind": "org",
        "invite_id": invite.id,
        "org_id": invite.org_id,
        "org": org_name,
        "message": format!("You've been invited to {org_name}"),
    });
    messenger
        .send(&toast, CHANNEL, &[Via::Websocket], &to)
        .await;
}
