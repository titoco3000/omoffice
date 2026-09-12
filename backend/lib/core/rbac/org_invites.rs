//! Org invitations: a pending row that only becomes membership on accept.
//! Delivery is always an email -- no "is this person online" branch.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{is_known_role, normalize_email, RbacError};
use crate::core::auth;
use crate::core::communication::email;
use crate::core::data::db::{retry_on_conflict, Db};

#[derive(Debug, Serialize)]
pub struct OrgInvite {
    pub id: String,
    pub org_id: String,
    pub email: String,
    pub roles: Vec<String>,
    pub invited_by: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_status_change: DateTime<Utc>,
}

/// `org_invites.roles` on disk, as the comma-joined string [`schema`](crate::schema)
/// declares it.
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
    last_status_change: DateTime<Utc>,
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
            last_status_change: row.last_status_change,
        }
    }
}

async fn get_invite(db: &Db, invite_id: &str) -> Result<Option<OrgInvite>, RbacError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, OrgInviteRow>(
        "SELECT id, org_id, email, roles, invited_by, status, created_at, last_status_change
         FROM org_invites WHERE id = $1",
    )
    .bind(invite_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(OrgInvite::from))
}

#[derive(sqlx::FromRow)]
struct InvitePreviewRow {
    id: String,
    org_id: String,
    org_name: String,
    email: String,
    roles: String,
    status: String,
}

/// What an invited email sees before deciding: the org's name, not just its
/// id, and no invite id-scoped auth beyond "you hold a bearer token" -- the
/// same magic link that logged them in is the only thing that named this id.
#[derive(Debug, Serialize)]
pub struct InvitePreview {
    pub id: String,
    pub org_id: String,
    pub org_name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub status: String,
}

pub async fn preview_invite(db: &Db, invite_id: &str) -> Result<InvitePreview, RbacError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, InvitePreviewRow>(
        "SELECT i.id, i.org_id, o.name AS org_name, i.email, i.roles, i.status
         FROM org_invites i
         JOIN organizations o ON o.id = i.org_id
         WHERE i.id = $1",
    )
    .bind(invite_id)
    .fetch_optional(pool)
    .await?
    .ok_or(RbacError::InviteNotFound)?;
    Ok(InvitePreview {
        id: row.id,
        org_id: row.org_id,
        org_name: row.org_name,
        email: row.email,
        roles: split_roles(&row.roles),
        status: row.status,
    })
}

/// Create a pending invite and email it. Reuses an existing pending row for
/// the same `(org_id, email)` rather than piling up duplicates.
pub async fn create_invite(
    db: &Db,
    org_id: &str,
    email_addr: &str,
    roles: &[String],
    invited_by: &str,
) -> Result<OrgInvite, RbacError> {
    for role in roles {
        if !is_known_role(role) {
            return Err(RbacError::UnknownRole);
        }
    }
    let email_addr = normalize_email(email_addr);
    let roles_joined = join_roles(roles);
    let pool = db.pool().await?;

    let existing_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM org_invites WHERE org_id = $1 AND email = $2 AND status = 'pending'",
    )
    .bind(org_id)
    .bind(&email_addr)
    .fetch_optional(pool)
    .await?;

    let id = match existing_id {
        Some(id) => {
            retry_on_conflict(|| {
                let id = id.clone();
                let roles_joined = roles_joined.clone();
                async move {
                    sqlx::query(
                        "UPDATE org_invites SET roles = $1, invited_by = $2, \
                         created_at = now(), last_status_change = now() WHERE id = $3",
                    )
                    .bind(&roles_joined)
                    .bind(invited_by)
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
                let email_addr = email_addr.clone();
                let roles_joined = roles_joined.clone();
                async move {
                    sqlx::query(
                        "INSERT INTO org_invites (id, org_id, email, roles, invited_by)
                         VALUES ($1, $2, $3, $4, $5)",
                    )
                    .bind(&id)
                    .bind(org_id)
                    .bind(&email_addr)
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

    let invite = get_invite(db, &id).await?.ok_or(RbacError::InviteNotFound)?;

    // The link logs the invited email in, same as a `request_key` one --
    // clicking it shouldn't leave them stuck asking for a second sign-in
    // before they can even see (let alone accept) what they were invited to.
    let user_id = super::register_user(db, &invite.email, None).await?;
    let login_token =
        auth::issue(&user_id).expect("HS256 JWT encoding does not fail for a well-formed payload");
    let magic_link = format!(
        "{}/?login={login_token}&invite=org:{}",
        crate::core::frontend_base_url(),
        invite.id
    );
    email::send_email(
        "noreply@omoffice.example",
        &invite.email,
        "You've been invited to an organization",
        &magic_link,
    );

    Ok(invite)
}

/// Accept a pending invite: mark it accepted, then [`super::add_member`] and
/// [`super::assign_role`] for each of its roles. The invited email may not
/// have a `users` row yet -- [`super::register_user`] resolves (or creates)
/// one, same as the magic-link sign-in path does.
pub async fn accept_invite(db: &Db, invite_id: &str) -> Result<OrgInvite, RbacError> {
    let invite = get_invite(db, invite_id)
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
                "UPDATE org_invites SET status = 'accepted', last_status_change = now() \
                 WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;

    let user_id = super::register_user(db, &invite.email, None).await?;
    super::add_member(db, &invite.org_id, &user_id).await?;
    for role in &invite.roles {
        super::assign_role(db, &invite.org_id, &user_id, role).await?;
    }
    Ok(invite)
}

pub async fn decline_invite(db: &Db, invite_id: &str) -> Result<(), RbacError> {
    let invite = get_invite(db, invite_id)
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
                "UPDATE org_invites SET status = 'declined', last_status_change = now() \
                 WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// `org_id` is checked so a `team-management` holder in one org can't revoke
/// another org's invite by id.
pub async fn revoke_invite(db: &Db, org_id: &str, invite_id: &str) -> Result<(), RbacError> {
    let invite = get_invite(db, invite_id)
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
            sqlx::query(
                "UPDATE org_invites SET status = 'revoked', last_status_change = now() \
                 WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

pub async fn list_invites(db: &Db, org_id: &str) -> Result<Vec<OrgInvite>, RbacError> {
    let pool = db.pool().await?;
    let rows: Vec<OrgInviteRow> = sqlx::query_as(
        "SELECT id, org_id, email, roles, invited_by, status, created_at, last_status_change
         FROM org_invites WHERE org_id = $1 AND status = 'pending' ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(OrgInvite::from).collect())
}
