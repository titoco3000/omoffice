//! Role-based access control: organizations and per-user roles within them.
//! [`require_role`]/[`require_member`] are the gates other modules call.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;

use crate::core::db::{Db, DbError};

pub mod invites;
pub mod orgs;
pub mod routes;

pub use invites::*;
pub use routes::*;

/// The roles native to the RBAC. A project's own roles (see
/// [`RolesConfig::project_roles`]) are plain strings, not part of this list.
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const BILLING: &str = "billing";
    pub const TEAM_MANAGEMENT: &str = "team-management";
    /// A member row with no explicit role — never a project role, never
    /// surfaced by a role picker.
    pub const DEFAULT: &str = "";
}

/// Builder for the role names a project allows, mirroring
/// [`crate::auth::AuthConfig`]'s shape: `RolesConfig::new().project_roles([...]).build()`.
pub struct RolesConfig {
    project_roles: Vec<String>,
}

impl RolesConfig {
    pub fn new() -> Self {
        Self {
            project_roles: Vec::new(),
        }
    }

    pub fn project_roles<I, S>(mut self, roles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.project_roles = roles.into_iter().map(Into::into).collect();
        self
    }

    pub fn build(self) -> AllowedRoles {
        let mut known = vec![
            roles::ADMIN.to_string(),
            roles::BILLING.to_string(),
            roles::TEAM_MANAGEMENT.to_string(),
            roles::DEFAULT.to_string(),
        ];
        known.extend(self.project_roles);
        AllowedRoles { known }
    }
}

impl Default for RolesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The native roles plus a project's own — the only strings `add_member`/
/// `replace_roles` accept. Cheap to clone, handed to handlers as an `Extension`.
#[derive(Clone)]
pub struct AllowedRoles {
    known: Vec<String>,
}

impl AllowedRoles {
    fn contains(&self, role: &str) -> bool {
        self.known.iter().any(|allowed| allowed == role)
    }

    /// `roles::DEFAULT` when `roles` is empty (a bare invite still leaves a
    /// row), otherwise `roles` unchanged.
    fn with_default_role(&self, roles: &[String]) -> Vec<String> {
        if roles.is_empty() {
            vec![roles::DEFAULT.to_string()]
        } else {
            roles.to_vec()
        }
    }
}

/// One row of `organizations`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Org {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// Everything an RBAC route can fail with.
#[derive(Debug)]
pub enum RbacError {
    Forbidden,
    OrgNotFound,
    MemberNotFound,
    /// The org would be left with no admin at all.
    LastAdmin,
    /// A role name outside the native three plus the project's own list.
    UnknownRole,
    InviteNotFound,
    /// The invite isn't `pending` any more; accept/revoke/decline all refuse
    /// to run twice.
    InviteNotPending,
    Database(DbError),
    Auth(crate::auth::AuthError),
}

impl From<DbError> for RbacError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

impl From<crate::auth::AuthError> for RbacError {
    fn from(err: crate::auth::AuthError) -> Self {
        Self::Auth(err)
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
                "that email is not a member of this organization".to_string(),
            ),
            Self::LastAdmin => (
                StatusCode::CONFLICT,
                "last_admin_cannot_leave",
                "an organization must keep at least one admin".to_string(),
            ),
            Self::UnknownRole => (
                StatusCode::BAD_REQUEST,
                "unknown_role",
                "that role is not native to the RBAC and not in this project's role list"
                    .to_string(),
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
            Self::Auth(err) => return err.into_response(),
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

async fn get_org(db: &Db, org_id: &str) -> Result<Option<Org>, RbacError> {
    let pool = db.pool().await?;
    let row = sqlx::query_as::<_, Org>(
        "SELECT id, name, created_by, created_at FROM organizations WHERE id = $1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

async fn has_exact_role(db: &Db, org_id: &str, email: &str, role: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let row: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM org_roles WHERE org_id = $1 AND user_email = $2 AND role = $3",
    )
    .bind(org_id)
    .bind(email)
    .bind(role)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// `admin` also satisfies a `team-management`/`billing` check — creating an
/// org only ever grants its creator `admin`, not all three as separate rows.
pub async fn has_role(db: &Db, email: &str, org_id: &str, role: &str) -> Result<bool, RbacError> {
    let email = normalize_email(email);
    if has_exact_role(db, org_id, &email, role).await? {
        return Ok(true);
    }
    if role == roles::TEAM_MANAGEMENT || role == roles::BILLING {
        return has_exact_role(db, org_id, &email, roles::ADMIN).await;
    }
    Ok(false)
}

/// The generic "does this user hold this role" gate: call before proceeding
/// with whatever the role protects.
pub async fn require_role(db: &Db, email: &str, org_id: &str, role: &str) -> Result<(), RbacError> {
    if has_role(db, email, org_id, role).await? {
        Ok(())
    } else {
        Err(RbacError::Forbidden)
    }
}

/// Any role at all — gates read access to a roster to members only, and is
/// how `chat` gates joining an org's room (chat has no roles of its own).
pub async fn require_member(db: &Db, email: &str, org_id: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    let row: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM org_roles WHERE org_id = $1 AND user_email = $2")
            .bind(org_id)
            .bind(normalize_email(email))
            .fetch_optional(pool)
            .await?;
    if row.is_some() {
        Ok(())
    } else {
        Err(RbacError::Forbidden)
    }
}

/// True if `email` is the only `admin` of `org_id`.
async fn is_last_admin(db: &Db, org_id: &str, email: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let admins: Vec<String> =
        sqlx::query_scalar("SELECT user_email FROM org_roles WHERE org_id = $1 AND role = $2")
            .bind(org_id)
            .bind(roles::ADMIN)
            .fetch_all(pool)
            .await?;
    Ok(admins.len() == 1 && admins[0] == normalize_email(email))
}
