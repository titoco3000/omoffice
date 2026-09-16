//! Role-based access control: organizations, their members, and the roles
//! each member holds. [`require_role`] / [`require_member`] are the gates
//! other modules call before doing anything org-scoped.
//!
//! Membership and permission are deliberately two different tables:
//! [`ORG_USERS`](crate::schema::ORG_USERS) is what makes someone a member
//! (and carries their per-org display name and online flag);
//! [`ORG_ROLES`](crate::schema::ORG_ROLES) is which extra permissions that
//! membership carries on top. Leaving an org clears both; a role change only
//! touches the second, and an admin may only leave (or be demoted out of)
//! `admin` if another admin remains -- see [`leave_org`].
//!
//! `org_users.online` is scoped to one org at a time, not to a user's whole
//! session: a browser tab looks at one org's panel at once, so a connection
//! is "online" in whichever org it most recently named, never in every org
//! that user happens to belong to. [`register`] tracks that per-user via the
//! messenger's [`CONNECTION`](web_messenger::CONNECTION) channel (a
//! disconnect clears whatever org was current) and [`ACTIVE_CHANNEL`] (the
//! frontend reports a switch), without the messenger itself knowing what an
//! org is.
//!
//! [`CHANNEL`] is the other direction: whenever a role changes, someone
//! joins or leaves, or a member's online org changes, [`broadcast_roster`]
//! pushes the fresh roster to every member of that org with an open socket,
//! so every tab open on it updates without polling. A member who was just
//! removed is no longer in that roster to receive the push, so
//! [`notify_removed`] tells them directly.

pub mod org_invites;
pub use org_invites::*;

use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::core::auth::Claims;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::{retry_on_conflict, Db, DbError};

/// The roles native to the RBAC. A member with none of these still has a
/// row in `org_users` -- membership and role are separate, see the module doc.
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const BILLING: &str = "billing";
    pub const TEAM_MANAGEMENT: &str = "team-management";
}

fn is_known_role(role: &str) -> bool {
    role == roles::ADMIN || role == roles::BILLING || role == roles::TEAM_MANAGEMENT
}

/// Carries an org's roster out to its members -- see the module doc. Frame
/// shapes: `{"org_id", "members": [OrgMember, ...]}` from [`broadcast_roster`],
/// or `{"org_id", "removed_you": true}` from [`notify_removed`].
pub const CHANNEL: web_messenger::Channel = "org_roster";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Org {
    pub id: String,
    pub name: String,
    pub created_by: String,
    /// S3 key of the org's icon
    pub picture: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One row of `org_users`, folded with that member's `org_roles`.
#[derive(Debug, Serialize)]
pub struct OrgMember {
    pub user_id: String,
    /// Org name, else global name, else email, else the id itself -- see
    /// [`display_name`].
    pub display_name: String,
    /// Org-specific picture, else the global one, else `None` -- see
    /// [`picture_key`].
    pub picture: Option<String>,
    /// Whether `picture` above is this member's own org-specific upload
    /// rather than the global picture shown through as a fallback --
    /// distinguishes "no override, showing the global photo" from "this
    /// person's photo in this org" for a UI (`Profile.jsx`'s "override
    /// photo in `<org>`" prompt) that needs to tell the two apart, since
    /// `picture` alone can't.
    pub has_own_picture: bool,
    pub email: Option<String>,
    pub online: bool,
    pub roles: Vec<String>,
}

/// Org name, else global name, else email, else `user_id` itself -- the
/// order the roster shows a member by. Blank strings count as missing, same
/// as `NULL`.
fn display_name(
    org_name: &Option<String>,
    global_name: &Option<String>,
    email: &Option<String>,
    user_id: &str,
) -> String {
    let present = |value: &Option<String>| {
        value
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .map(String::from)
    };
    present(org_name)
        .or_else(|| present(global_name))
        .or_else(|| present(email))
        .unwrap_or_else(|| user_id.to_string())
}

/// Org-specific picture, else the global one -- same override pattern as
/// [`display_name`], minus the email/id fallbacks (no picture is a valid
/// end state, unlike no name).
fn picture_key(org_picture: &Option<String>, global_picture: &Option<String>) -> Option<String> {
    let present = |value: &Option<String>| {
        value
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .map(String::from)
    };
    present(org_picture).or_else(|| present(global_picture))
}

#[derive(Debug)]
pub enum RbacError {
    Forbidden,
    OrgNotFound,
    MemberNotFound,
    /// The org would be left with no admin at all.
    LastAdmin,
    UnknownRole,
    InviteNotFound,
    InviteNotPending,
    Database(DbError),
}

impl From<DbError> for RbacError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

/// So a plain `sqlx::query...(...).await?` composes inside a function
/// returning `Result<_, RbacError>`.
impl From<sqlx::Error> for RbacError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(DbError::from(err))
    }
}

impl IntoResponse for RbacError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "role_required",
                "you do not hold the role this action requires".to_string(),
            ),
            Self::OrgNotFound => (
                StatusCode::NOT_FOUND,
                "org_not_found",
                "no such organization".to_string(),
            ),
            Self::MemberNotFound => (
                StatusCode::NOT_FOUND,
                "member_not_found",
                "that user is not a member of this organization".to_string(),
            ),
            Self::LastAdmin => (
                StatusCode::CONFLICT,
                "last_admin_cannot_leave",
                "an organization must keep at least one admin".to_string(),
            ),
            Self::UnknownRole => (
                StatusCode::BAD_REQUEST,
                "unknown_role",
                "that role is not native to the RBAC".to_string(),
            ),
            Self::InviteNotFound => (
                StatusCode::NOT_FOUND,
                "invite_not_found",
                "no such invite".to_string(),
            ),
            Self::InviteNotPending => (
                StatusCode::CONFLICT,
                "invite_not_pending",
                "this invite has already been accepted, revoked or declined".to_string(),
            ),
            Self::Database(err) => return err.into_response(),
        };
        (status, Json(json!({ "error": error, "message": message }))).into_response()
    }
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

async fn org_exists(db: &Db, org_id: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let row: Option<i32> = sqlx::query_scalar("SELECT 1 FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

async fn is_member(db: &Db, org_id: &str, user_id: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let row: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM org_users WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

async fn has_exact_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let row: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM org_roles WHERE org_id = $1 AND user_id = $2 AND role = $3",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(role)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// `admin` also satisfies a `team-management`/`billing` check -- creating an
/// org only ever grants its creator `admin`, not all three as separate rows.
async fn has_role(db: &Db, org_id: &str, user_id: &str, role: &str) -> Result<bool, RbacError> {
    if has_exact_role(db, org_id, user_id, role).await? {
        return Ok(true);
    }
    if role == roles::TEAM_MANAGEMENT || role == roles::BILLING {
        return has_exact_role(db, org_id, user_id, roles::ADMIN).await;
    }
    Ok(false)
}

/// True if `user_id` is the only `admin` of `org_id`.
async fn is_last_admin(db: &Db, org_id: &str, user_id: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let admins: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM org_roles WHERE org_id = $1 AND role = $2")
            .bind(org_id)
            .bind(roles::ADMIN)
            .fetch_all(pool)
            .await?;
    Ok(admins.len() == 1 && admins[0] == user_id)
}

/// Find or create the `users` row for `email`, returning its id. Not
/// airtight against two callers racing to register the same brand-new
/// email at once -- `email` carries no unique constraint the ORM can
/// declare (only a primary key can be fixed at `CREATE TABLE` time) -- but
/// that's the same race every magic-link request already runs.
pub async fn register_user(db: &Db, email: &str, name: Option<&str>) -> Result<String, RbacError> {
    let email = normalize_email(email);
    let pool = db.pool().await?;

    if let Some(id) = sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(pool)
        .await?
    {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    let name = name.map(str::to_string);
    retry_on_conflict(|| {
        let id = id.clone();
        let email = email.clone();
        let name = name.clone();
        async move {
            sqlx::query("INSERT INTO users (id, email, name) VALUES ($1, $2, $3)")
                .bind(&id)
                .bind(&email)
                .bind(&name)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(id)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    /// S3 key of this user's global avatar, or `None` for none.
    pub picture: Option<String>,
}

pub async fn get_user(db: &Db, user_id: &str) -> Result<Option<User>, RbacError> {
    let pool = db.pool().await?;
    let user =
        sqlx::query_as::<_, User>("SELECT id, email, name, picture FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(user)
}

/// Create an org. The creator is always its first `admin` -- an org can
/// never exist with zero admins.
pub async fn create_org(db: &Db, name: &str, creator: &str) -> Result<Org, RbacError> {
    let id = Uuid::new_v4().to_string();
    let pool = db.pool().await?;
    let org = retry_on_conflict(|| {
        let id = id.clone();
        async move {
            sqlx::query_as::<_, Org>(
                "INSERT INTO organizations (id, name, created_by) VALUES ($1, $2, $3)
                 RETURNING id, name, created_by, picture, created_at",
            )
            .bind(&id)
            .bind(name)
            .bind(creator)
            .fetch_one(pool)
            .await
        }
    })
    .await?;

    add_member(db, &org.id, creator).await?;
    assign_role(db, &org.id, creator, roles::ADMIN).await?;
    Ok(org)
}

/// Delete an org: everything with a foreign key into `organizations` (and,
/// transitively, into its `meetings`) first (DSQL has no cascading deletes),
/// then the `organizations` row itself.
pub async fn delete_org(db: &Db, org_id: &str) -> Result<(), RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let pool = db.pool().await?;
    // meeting_rooms.assigned_to only ever references a meeting, never an
    // org -- unassign rather than delete so pooled rooms stay reusable.
    retry_on_conflict(|| async move {
        sqlx::query(
            "UPDATE meeting_rooms SET assigned_to = NULL \
             WHERE assigned_to IN (SELECT id FROM meetings WHERE org_id = $1)",
        )
        .bind(org_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "DELETE FROM meeting_attendees \
             WHERE meeting_id IN (SELECT id FROM meetings WHERE org_id = $1)",
        )
        .bind(org_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "DELETE FROM meeting_invites \
             WHERE meeting_id IN (SELECT id FROM meetings WHERE org_id = $1)",
        )
        .bind(org_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM meetings WHERE org_id = $1")
            .bind(org_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_invites WHERE org_id = $1")
            .bind(org_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_roles WHERE org_id = $1")
            .bind(org_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_users WHERE org_id = $1")
            .bind(org_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(org_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn rename_org(db: &Db, org_id: &str, name: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    let result = retry_on_conflict(|| async move {
        sqlx::query("UPDATE organizations SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(org_id)
            .execute(pool)
            .await
    })
    .await?;
    if result.rows_affected() == 0 {
        return Err(RbacError::OrgNotFound);
    }
    Ok(())
}

/// Renames how `user_id` appears inside `org_id` only (`org_users.name`).
pub async fn rename_in_org(
    db: &Db,
    org_id: &str,
    user_id: &str,
    name: &str,
) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    let result = retry_on_conflict(|| async move {
        sqlx::query("UPDATE org_users SET name = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(name)
            .bind(org_id)
            .bind(user_id)
            .execute(pool)
            .await
    })
    .await?;
    if result.rows_affected() == 0 {
        return Err(RbacError::MemberNotFound);
    }
    Ok(())
}

/// Renames `user_id` everywhere (`users.name`).
pub async fn rename_globally(db: &Db, user_id: &str, name: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE users SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn mark_upload_pending(db: &Db, user_id: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE users SET pending_upload_at = now() WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn clear_upload_pending(db: &Db, user_id: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE users SET pending_upload_at = NULL WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn upload_pending(db: &Db, user_id: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let at: Option<Option<DateTime<Utc>>> =
        sqlx::query_scalar("SELECT pending_upload_at FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(at.flatten().is_some())
}

/// Sets `key` as `user_id`'s global avatar -- `users.picture` -- or clears
/// it (`None`) when the user removes their photo.
pub async fn set_picture_globally(
    db: &Db,
    user_id: &str,
    key: Option<&str>,
) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE users SET picture = $1 WHERE id = $2")
            .bind(key)
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

/// Sets `key` as how `user_id` appears inside `org_id` only --
/// `org_users.picture` -- or clears it (`None`).
pub async fn set_picture_in_org(
    db: &Db,
    org_id: &str,
    user_id: &str,
    key: Option<&str>,
) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    let result = retry_on_conflict(|| async move {
        sqlx::query("UPDATE org_users SET picture = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(key)
            .bind(org_id)
            .bind(user_id)
            .execute(pool)
            .await
    })
    .await?;
    if result.rows_affected() == 0 {
        return Err(RbacError::MemberNotFound);
    }
    Ok(())
}

/// Sets `key` as `org_id`'s icon -- `organizations.picture` -- or clears it
/// (`None`).
pub async fn set_org_picture(db: &Db, org_id: &str, key: Option<&str>) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    let result = retry_on_conflict(|| async move {
        sqlx::query("UPDATE organizations SET picture = $1 WHERE id = $2")
            .bind(key)
            .bind(org_id)
            .execute(pool)
            .await
    })
    .await?;
    if result.rows_affected() == 0 {
        return Err(RbacError::OrgNotFound);
    }
    Ok(())
}

/// The raw `org_users.name` for one member -- no fallback to the global
/// name, unlike [`OrgMember::display_name`]. Callers use this to tell
/// whether a rename is actually effective before writing it and pushing a
/// roster broadcast; a row with no name and no row at all both come back as
/// `None`, so the caller must already have established membership (e.g. via
/// [`require_member`]) before trusting that.
pub async fn member_name(
    db: &Db,
    org_id: &str,
    user_id: &str,
) -> Result<Option<String>, RbacError> {
    let pool = db.pool().await?;
    let name: Option<Option<String>> =
        sqlx::query_scalar("SELECT name FROM org_users WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(name.flatten())
}

/// The raw `org_users.picture` for one member -- no fallback to the global
/// picture, unlike [`OrgMember::picture`](OrgMember). Used to find what a
/// confirmed upload is about to replace.
pub async fn member_picture(
    db: &Db,
    org_id: &str,
    user_id: &str,
) -> Result<Option<String>, RbacError> {
    let pool = db.pool().await?;
    let picture: Option<Option<String>> =
        sqlx::query_scalar("SELECT picture FROM org_users WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(picture.flatten())
}

/// The raw `organizations.picture` for one org.
pub async fn org_picture(db: &Db, org_id: &str) -> Result<Option<String>, RbacError> {
    let pool = db.pool().await?;
    let picture: Option<Option<String>> =
        sqlx::query_scalar("SELECT picture FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_optional(pool)
            .await?;
    Ok(picture.flatten())
}

/// One org in `GET /rbac/orgs`, the caller's own roles folded in.
#[derive(Debug, Serialize)]
pub struct OrgSummary {
    pub id: String,
    pub name: String,
    pub picture: Option<String>,
    pub roles: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct OrgRoleRow {
    id: String,
    name: String,
    picture: Option<String>,
    role: Option<String>,
}

/// The orgs `user_id` belongs to, each with their own roles in it folded in.
pub async fn list_orgs_for_user(db: &Db, user_id: &str) -> Result<Vec<OrgSummary>, RbacError> {
    let pool = db.pool().await?;
    let rows: Vec<OrgRoleRow> = sqlx::query_as(
        "SELECT o.id, o.name, o.picture, r.role
         FROM org_users u
         JOIN organizations o ON o.id = u.org_id
         LEFT JOIN org_roles r ON r.org_id = u.org_id AND r.user_id = u.user_id
         WHERE u.user_id = $1
         ORDER BY o.id, r.role",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let mut orgs: Vec<OrgSummary> = Vec::new();
    for row in rows {
        match orgs.last_mut() {
            Some(last) if last.id == row.id => {
                if let Some(role) = row.role {
                    last.roles.push(role);
                }
            }
            _ => orgs.push(OrgSummary {
                id: row.id,
                name: row.name,
                picture: row.picture,
                roles: row.role.into_iter().collect(),
            }),
        }
    }
    Ok(orgs)
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    user_id: String,
    org_name: Option<String>,
    global_name: Option<String>,
    org_picture: Option<String>,
    global_picture: Option<String>,
    email: Option<String>,
    online: Option<bool>,
    role: Option<String>,
}

/// The full roster of `org_id`, each member's roles folded in.
pub async fn list_members(db: &Db, org_id: &str) -> Result<Vec<OrgMember>, RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let pool = db.pool().await?;
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT u.user_id, u.name AS org_name, usr.name AS global_name,
                u.picture AS org_picture, usr.picture AS global_picture,
                usr.email, u.online, r.role
         FROM org_users u
         JOIN users usr ON usr.id = u.user_id
         LEFT JOIN org_roles r ON r.org_id = u.org_id AND r.user_id = u.user_id
         WHERE u.org_id = $1
         ORDER BY u.user_id, r.role",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;

    let mut members: Vec<OrgMember> = Vec::new();
    for row in rows {
        match members.last_mut() {
            Some(last) if last.user_id == row.user_id => {
                if let Some(role) = row.role {
                    last.roles.push(role);
                }
            }
            _ => {
                let name = display_name(&row.org_name, &row.global_name, &row.email, &row.user_id);
                let has_own_picture = row
                    .org_picture
                    .as_ref()
                    .is_some_and(|s| !s.trim().is_empty());
                let picture = picture_key(&row.org_picture, &row.global_picture);
                members.push(OrgMember {
                    user_id: row.user_id,
                    display_name: name,
                    picture,
                    has_own_picture,
                    email: row.email,
                    online: row.online.unwrap_or(false),
                    roles: row.role.into_iter().collect(),
                })
            }
        }
    }
    Ok(members)
}

/// Push `org_id`'s current roster to every member of it who is currently
/// online, on [`CHANNEL`] -- an offline member has no open socket to reach
/// anyway, and will see the fresh roster the next time they load it. The
/// payload itself still lists every member, online or not, same as
/// [`list_members`] -- only who it's *sent to* is narrowed. Call this after
/// any change to membership or roles.
pub async fn broadcast_roster(
    messenger: &Messenger,
    db: &Db,
    org_id: &str,
) -> Result<(), RbacError> {
    let members = list_members(db, org_id).await?;
    let online_ids: Vec<String> = members
        .iter()
        .filter(|member| member.online)
        .map(|member| member.user_id.clone())
        .collect();
    messenger
        .send(
            CHANNEL,
            &online_ids,
            json!({ "org_id": org_id, "members": members }),
        )
        .await;
    Ok(())
}

/// Tell `user_id` specifically that they no longer belong to `org_id`, on
/// [`CHANNEL`] -- call this alongside [`broadcast_roster`] whenever a
/// removal leaves the removed member out of that roster to receive it.
pub async fn notify_removed(messenger: &Messenger, org_id: &str, user_id: &str) {
    messenger
        .send(
            CHANNEL,
            &[user_id.to_string()],
            json!({ "org_id": org_id, "removed_you": true }),
        )
        .await;
}

/// Add `user_id` to `org_id` -- a bare membership row, no roles yet. What an
/// accepted invite calls before [`assign_role`] for each invited role.
pub async fn add_member(db: &Db, org_id: &str, user_id: &str) -> Result<(), RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO org_users (org_id, user_id) VALUES ($1, $2)
             ON CONFLICT (org_id, user_id) DO NOTHING",
        )
        .bind(org_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn assign_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), RbacError> {
    if !is_known_role(role) {
        return Err(RbacError::UnknownRole);
    }
    if !is_member(db, org_id, user_id).await? {
        return Err(RbacError::MemberNotFound);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO org_roles (org_id, user_id, role) VALUES ($1, $2, $3)
             ON CONFLICT (org_id, user_id, role) DO NOTHING",
        )
        .bind(org_id)
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn revoke_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), RbacError> {
    if !is_member(db, org_id, user_id).await? {
        return Err(RbacError::MemberNotFound);
    }
    if role == roles::ADMIN && is_last_admin(db, org_id, user_id).await? {
        return Err(RbacError::LastAdmin);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_roles WHERE org_id = $1 AND user_id = $2 AND role = $3")
            .bind(org_id)
            .bind(user_id)
            .bind(role)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

/// Remove `user_id` from `org_id` entirely -- both `org_users` and
/// `org_roles`. Refuses if they are the org's last admin; see the module doc.
pub async fn leave_org(db: &Db, org_id: &str, user_id: &str) -> Result<(), RbacError> {
    if !is_member(db, org_id, user_id).await? {
        return Err(RbacError::MemberNotFound);
    }
    if is_last_admin(db, org_id, user_id).await? {
        return Err(RbacError::LastAdmin);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_roles WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_users WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

/// The generic "does this user hold this role" gate.
pub async fn require_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), RbacError> {
    if has_role(db, org_id, user_id, role).await? {
        Ok(())
    } else {
        Err(RbacError::Forbidden)
    }
}

/// Any membership at all, no particular role -- gates a roster read to
/// members only.
pub async fn require_member(db: &Db, org_id: &str, user_id: &str) -> Result<(), RbacError> {
    if is_member(db, org_id, user_id).await? {
        Ok(())
    } else {
        Err(RbacError::Forbidden)
    }
}

/// `org_users.online` is per `(org_id, user_id)` -- see the module doc --
/// so this only ever touches the one row for `org_id`, never every org
/// `user_id` belongs to.
async fn set_online(db: &Db, org_id: &str, user_id: &str, online: bool) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("UPDATE org_users SET online = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(online)
            .bind(org_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

/// The org `user_id`'s connection currently reports as active, if any --
/// read straight off `org_users.online` rather than kept a second time in
/// some in-process place. [`register`] only ever flips one org's flag on
/// after flipping the previous one off, so at most one row is ever `online`
/// for a given user, and this is exact even across separate Lambda
/// invocations sharing no memory with each other.
async fn active_org(db: &Db, user_id: &str) -> Result<Option<String>, RbacError> {
    let pool = db.pool().await?;
    let org_id: Option<String> = sqlx::query_scalar(
        "SELECT org_id FROM org_users WHERE user_id = $1 AND online = true LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(org_id)
}

/// [`set_online`] then [`broadcast_roster`], logging rather than failing --
/// there is no request here to hand a `RbacError` back to.
async fn set_online_and_broadcast(
    messenger: &Messenger,
    db: &Db,
    org_id: &str,
    user_id: &str,
    online: bool,
) {
    if let Err(err) = set_online(db, org_id, user_id, online).await {
        tracing::warn!("rbac: could not set online={online} for {user_id} in {org_id}: {err:?}");
        return;
    }
    if let Err(err) = broadcast_roster(messenger, db, org_id).await {
        tracing::warn!("rbac: could not broadcast roster for org {org_id}: {err:?}");
    }
}

/// Carries which org a connection is currently looking at: `{"org_id": ..}`,
/// or `{"org_id": null}` for none. Sent by the frontend whenever the
/// selected org changes -- see the module doc for why "online" follows this
/// rather than plain membership.
pub const ACTIVE_CHANNEL: web_messenger::Channel = "org_active";

/// Wire `org_users.online` to the messenger: [`CONNECTION`](web_messenger::CONNECTION)
/// for a clean disconnect, [`ACTIVE_CHANNEL`] for which org a live
/// connection currently means. Both handlers read the current active org
/// straight off `org_users.online` (via [`active_org`]) rather than keeping
/// a second, in-process copy of it -- that copy would be per Lambda
/// invocation, not per user, and so would not survive past the invocation
/// that set it. Meant to be called once, at startup.
pub fn register(messenger: &Messenger, db: Db) {
    {
        let db = db.clone();
        messenger.on_message(
            web_messenger::CONNECTION,
            move |messenger, user_id, message| {
                let db = db.clone();
                async move {
                    if message.as_str() != Some(web_messenger::DISCONNECTED) {
                        // CONNECTED alone names no org yet -- that arrives
                        // separately on `ACTIVE_CHANNEL`, once the frontend knows.
                        return;
                    }
                    match active_org(&db, &user_id).await {
                        Ok(Some(org_id)) => {
                            set_online_and_broadcast(&messenger, &db, &org_id, &user_id, false)
                                .await;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            tracing::warn!(
                                "rbac: could not look up {user_id}'s active org: {err:?}"
                            )
                        }
                    }
                }
            },
        );
    }

    messenger.on_message(ACTIVE_CHANNEL, move |messenger, user_id, message| {
        let db = db.clone();
        async move {
            let new_org = message
                .get("org_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let previous = match active_org(&db, &user_id).await {
                Ok(previous) => previous,
                Err(err) => {
                    tracing::warn!("rbac: could not look up {user_id}'s active org: {err:?}");
                    return;
                }
            };
            if previous == new_org {
                return;
            }
            if let Some(previous) = previous {
                set_online_and_broadcast(&messenger, &db, &previous, &user_id, false).await;
            }
            if let Some(new_org) = new_org {
                set_online_and_broadcast(&messenger, &db, &new_org, &user_id, true).await;
            }
        }
    });
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

#[derive(Deserialize)]
struct RoleBody {
    user_id: String,
    role: String,
}

#[derive(Deserialize)]
struct InviteBody {
    email: String,
    #[serde(default)]
    roles: Vec<String>,
}

async fn me_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let user = get_user(&db, &claims.sub)
        .await?
        .ok_or(RbacError::Forbidden)?;
    Ok(Json(json!(user)))
}

/// Renaming the account globally moves every org's `display_name` for it
/// that has no org-specific override -- so every org this user belongs to
/// gets its roster re-pushed, not just the one (if any) currently open.
///
/// Skipped entirely (no write, no broadcast) when the submitted name
/// matches what's already stored -- the frontend saves on every blur, not
/// just an actual edit, and a no-op rename shouldn't push a roster update
/// to every org this user is in.
async fn rename_me_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<NameBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let current = get_user(&db, &claims.sub).await?.and_then(|user| user.name);
    if current.as_deref() != Some(body.name.as_str()) {
        rename_globally(&db, &claims.sub, &body.name).await?;
        for org in list_orgs_for_user(&db, &claims.sub).await? {
            broadcast_roster(&messenger, &db, &org.id).await?;
        }
    }
    Ok(Json(json!({ "name": body.name })))
}

async fn rename_me_in_org_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<NameBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_member(&db, &org_id, &claims.sub).await?;
    let current = member_name(&db, &org_id, &claims.sub).await?;
    if current.as_deref() != Some(body.name.as_str()) {
        rename_in_org(&db, &org_id, &claims.sub, &body.name).await?;
        broadcast_roster(&messenger, &db, &org_id).await?;
    }
    Ok(Json(json!({ "org_id": org_id, "name": body.name })))
}

async fn list_orgs_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let orgs = list_orgs_for_user(&db, &claims.sub).await?;
    Ok(Json(json!({ "orgs": orgs })))
}

async fn create_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<NameBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let org = create_org(&db, &body.name, &claims.sub).await?;
    Ok(Json(json!({ "org": org })))
}

async fn rename_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<NameBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::ADMIN).await?;
    rename_org(&db, &org_id, &body.name).await?;
    Ok(Json(json!({ "org_id": org_id, "name": body.name })))
}

async fn delete_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::ADMIN).await?;
    delete_org(&db, &org_id).await?;
    Ok(Json(json!({ "deleted": org_id })))
}

async fn list_members_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_member(&db, &org_id, &claims.sub).await?;
    let members = list_members(&db, &org_id).await?;
    Ok(Json(json!({ "members": members })))
}

async fn leave_org_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    leave_org(&db, &org_id, &claims.sub).await?;
    notify_removed(&messenger, &org_id, &claims.sub).await;
    broadcast_roster(&messenger, &db, &org_id).await?;
    Ok(Json(json!({ "org_id": org_id, "left_by": claims.sub })))
}

/// Same removal as [`leave_org_route`], gated on `team-management` (which
/// `admin` also satisfies, see [`has_role`]) since it targets someone other
/// than the caller. [`leave_org`] itself still refuses to strip the org's
/// last admin, whoever asks.
async fn remove_member_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path((org_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::TEAM_MANAGEMENT).await?;
    leave_org(&db, &org_id, &user_id).await?;
    notify_removed(&messenger, &org_id, &user_id).await;
    broadcast_roster(&messenger, &db, &org_id).await?;
    Ok(Json(json!({ "org_id": org_id, "removed": user_id })))
}

async fn assign_role_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<RoleBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::TEAM_MANAGEMENT).await?;
    assign_role(&db, &org_id, &body.user_id, &body.role).await?;
    broadcast_roster(&messenger, &db, &org_id).await?;
    Ok(Json(
        json!({ "org_id": org_id, "user_id": body.user_id, "role": body.role }),
    ))
}

async fn revoke_role_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<RoleBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::TEAM_MANAGEMENT).await?;
    revoke_role(&db, &org_id, &body.user_id, &body.role).await?;
    broadcast_roster(&messenger, &db, &org_id).await?;
    Ok(Json(
        json!({ "org_id": org_id, "user_id": body.user_id, "role": body.role }),
    ))
}

async fn create_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(body): Json<InviteBody>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::TEAM_MANAGEMENT).await?;
    let invite = create_invite(&db, &org_id, &body.email, &body.roles, &claims.sub).await?;
    Ok(Json(json!({ "invite": invite })))
}

async fn list_invites_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::TEAM_MANAGEMENT).await?;
    let invites = list_invites(&db, &org_id).await?;
    Ok(Json(json!({ "invites": invites })))
}

async fn revoke_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path((org_id, invite_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, RbacError> {
    require_role(&db, &org_id, &claims.sub, roles::TEAM_MANAGEMENT).await?;
    revoke_invite(&db, &org_id, &invite_id).await?;
    Ok(Json(json!({ "revoked": invite_id })))
}

/// `GET /invites` -- every pending org invite addressed to the caller's own
/// email, across every org, regardless of whether they were online when it
/// was issued or even had an account yet.
async fn list_my_invites_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let invites = invites_for_user(&db, &claims.sub).await?;
    Ok(Json(json!({ "invites": invites })))
}

/// No further check beyond a valid bearer token -- same as accept/decline,
/// the invite id itself (only reachable from the magic link) is what gates
/// this, not the caller's own identity.
async fn invite_preview_route(
    Extension(db): Extension<Db>,
    Path(invite_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let invite = preview_invite(&db, &invite_id).await?;
    Ok(Json(json!({ "invite": invite })))
}

/// No further check beyond a valid bearer token -- the invited email is
/// whatever the invite itself names, not necessarily the caller's own.
async fn accept_invite_route(
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Path(invite_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    let invite = accept_invite(&db, &invite_id).await?;
    broadcast_roster(&messenger, &db, &invite.org_id).await?;
    Ok(Json(json!({ "invite": invite })))
}

async fn decline_invite_route(
    Extension(db): Extension<Db>,
    Path(invite_id): Path<String>,
) -> Result<Json<serde_json::Value>, RbacError> {
    decline_invite(&db, &invite_id).await?;
    Ok(Json(json!({ "declined": invite_id })))
}

/// `/rbac/*` -- org and membership management. Nested under
/// [`auth::require_bearer`](crate::core::auth::require_bearer), so every
/// handler here can pull `Extension<Claims>` for the caller's `user_id`.
pub fn routes() -> Router {
    Router::new()
        .route("/me", get(me_route))
        .route("/me/name", put(rename_me_route))
        .route("/orgs", get(list_orgs_route).post(create_org_route))
        .route("/orgs/{org_id}", delete(delete_org_route))
        .route("/orgs/{org_id}/name", put(rename_org_route))
        .route("/orgs/{org_id}/members", get(list_members_route))
        .route("/orgs/{org_id}/members/me", delete(leave_org_route))
        .route(
            "/orgs/{org_id}/members/me/name",
            put(rename_me_in_org_route),
        )
        .route(
            "/orgs/{org_id}/members/{user_id}",
            delete(remove_member_route),
        )
        .route(
            "/orgs/{org_id}/roles",
            post(assign_role_route).delete(revoke_role_route),
        )
        .route(
            "/orgs/{org_id}/invites",
            get(list_invites_route).post(create_invite_route),
        )
        .route(
            "/orgs/{org_id}/invites/{invite_id}",
            delete(revoke_invite_route),
        )
        .route("/invites", get(list_my_invites_route))
        .route("/invites/{invite_id}", get(invite_preview_route))
        .route("/invites/{invite_id}/accept", post(accept_invite_route))
        .route("/invites/{invite_id}/decline", post(decline_invite_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn display_name_prefers_org_name_over_everything_else() {
        assert_eq!(
            display_name(&some("Org Nick"), &some("Global"), &some("a@b.com"), "u1"),
            "Org Nick"
        );
    }

    #[test]
    fn display_name_falls_back_to_global_name() {
        assert_eq!(
            display_name(&None, &some("Global"), &some("a@b.com"), "u1"),
            "Global"
        );
    }

    #[test]
    fn display_name_falls_back_to_email() {
        assert_eq!(
            display_name(&None, &None, &some("a@b.com"), "u1"),
            "a@b.com"
        );
    }

    #[test]
    fn display_name_falls_back_to_user_id() {
        assert_eq!(display_name(&None, &None, &None, "u1"), "u1");
    }

    #[test]
    fn display_name_treats_blank_strings_as_missing() {
        assert_eq!(
            display_name(&some("   "), &some(""), &some("a@b.com"), "u1"),
            "a@b.com"
        );
    }
}
