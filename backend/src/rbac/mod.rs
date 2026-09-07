//! Project-agnostic role-based access control: organizations, and who holds
//! which role in which one.
//!
//! This module knows nothing about meetings or anything else the app does —
//! it only knows Users (by email, same as `users.rs`), Organizations, and a
//! user-org-role table (`org_roles`, one row per role a user holds in an
//! org, so a user can hold several at once). Any other module wanting to
//! gate a route behind a role calls [`require_role`]; `main.rs` is the only
//! place that ever names a project-specific role string like `"post"` — see
//! [`roles`] for the three that are native to the RBAC itself instead.
//!
//! Mechanisms: [`create_org`] (creator becomes `admin`), [`add_member`]
//! (invite and "add a user" are the same thing — it provisions the `users`
//! row if the email is new), [`replace_roles`] ("change user role"; revoking
//! down to no explicit role demotes to [`roles::DEFAULT`] rather than
//! dropping the member — [`leave_org`] is the actual "remove a member",
//! self-service and blocked for the last admin), [`delete_org`].

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{AuthError, AuthState, Claims};
use crate::core::apigw_management::ApiGatewayManagement;
use crate::core::db::{retry_on_conflict, Db, DbError};
use crate::messenger::Messenger;
use crate::users;

/// The roles native to the RBAC. Everything else is a plain string a project
/// defines for itself (see [`RolesConfig::project_roles`]) — there is no enum
/// of "all roles" here on purpose, since that list is per-project.
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const BILLING: &str = "billing";
    pub const TEAM_MANAGEMENT: &str = "team-management";
    /// The empty string: a row `org_roles` can hold for a member with no
    /// explicit role, so "is this email a member" is still just "does a row
    /// exist" — see [`AllowedRoles::with_default_role`]. Never a project
    /// role, never surfaced by a role picker.
    pub const DEFAULT: &str = "";
}

/// Builder for the full set of role names a `POST`/`PUT` to `/private/orgs/...`
/// may assign, mirroring [`crate::auth::AuthConfig`]'s shape: `app()` states
/// the project's own roles in one place, this module only enforces them.
///
/// ```ignore
/// RolesConfig::new().project_roles(["post"]).build()
/// ```
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

/// The native roles plus whatever a project added — the only role strings
/// `add_member`/`replace_roles` will accept. Cheap to clone, handed to
/// handlers as an `Extension`, same as `Db`/`ChimeState`.
#[derive(Clone)]
pub struct AllowedRoles {
    known: Vec<String>,
}

impl AllowedRoles {
    fn contains(&self, role: &str) -> bool {
        self.known.iter().any(|allowed| allowed == role)
    }

    /// `roles::DEFAULT` when `roles` is empty (so a bare invite or a full
    /// revoke still leaves a row in `org_roles`), otherwise `roles`
    /// unchanged — an explicit role already marks membership, so the default
    /// is never stacked alongside a real one. Callers granting real roles to
    /// an existing member are responsible for dropping that member's stale
    /// default row themselves; see [`add_member`].
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

/// One org in `GET /private/orgs`, with the caller's own roles folded in.
#[derive(Debug, Serialize)]
pub struct OrgSummary {
    pub id: String,
    pub name: String,
    pub roles: Vec<String>,
}

/// One member in `GET /private/orgs/{org_id}`.
#[derive(Debug, Serialize)]
pub struct MemberSummary {
    pub email: String,
    pub roles: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct OrgRoleRow {
    id: String,
    name: String,
    role: String,
}

#[derive(sqlx::FromRow)]
struct MemberRoleRow {
    email: String,
    role: String,
}

/// Everything an RBAC route can fail with.
#[derive(Debug)]
pub enum RbacError {
    /// The caller does not hold the required role (or any role at all, for a
    /// membership-only check).
    Forbidden,
    OrgNotFound,
    MemberNotFound,
    /// The org would be left with no admin at all.
    LastAdmin,
    /// A role name outside the native three plus the project's own list.
    UnknownRole,
    InviteNotFound,
    /// The invite exists but isn't `pending` any more (already accepted,
    /// revoked, or declined) -- accept/revoke/decline all refuse to run
    /// twice.
    InviteNotPending,
    Database(DbError),
    Auth(AuthError),
}

impl From<DbError> for RbacError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
    }
}

impl From<AuthError> for RbacError {
    fn from(err: AuthError) -> Self {
        Self::Auth(err)
    }
}

/// So a plain `sqlx::query...(...).await?` inside a function returning
/// `Result<_, RbacError>` composes, the same way `DbError`'s own
/// `From<sqlx::Error>` lets it compose one level down.
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

/// Single-row fetch by id -- `create_invite`/`accept_invite` need the org's
/// name and creation info, `list_orgs_for_user`/`org_detail_route` never
/// needed this before because they always fetch a roster, not one org.
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

/// Does `email` hold `role` in `org_id`? `admin` also satisfies a
/// `team-management` or `billing` check: the role that can delete the org
/// can also manage its membership and see its billing, so creating an org
/// only ever grants its creator `admin` — not all three native roles as
/// separate rows — and this is where that implication is honored. It never
/// extends to a project-specific role (e.g. `"post"`): those stay separately
/// granted, on purpose.
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

/// The generic "does this user hold this role" gate: call this before
/// proceeding with whatever the role protects. `Err(RbacError::Forbidden)`
/// short-circuits the caller with `?`.
pub async fn require_role(db: &Db, email: &str, org_id: &str, role: &str) -> Result<(), RbacError> {
    if has_role(db, email, org_id, role).await? {
        Ok(())
    } else {
        Err(RbacError::Forbidden)
    }
}

/// Any role at all — gates read access to a roster to members only. Also
/// used by `chat` to gate joining an org's room: chat has no roles of its
/// own, only membership.
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

/// True if `email` is the only `admin` of `org_id` — shared by the checks in
/// [`replace_roles`] and [`leave_org`].
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

/// Create an org. The creator is always its first `admin` — an org can never
/// be created with zero admins.
pub async fn create_org(db: &Db, name: &str, creator_email: &str) -> Result<Org, RbacError> {
    let creator_email = normalize_email(creator_email);
    let id = Uuid::new_v4().to_string();
    let pool = db.pool().await?;
    let org = retry_on_conflict(|| {
        let id = id.clone();
        let creator_email = creator_email.clone();
        async move {
            sqlx::query_as::<_, Org>(
                "INSERT INTO organizations (id, name, created_by) VALUES ($1, $2, $3)
                 RETURNING id, name, created_by, created_at",
            )
            .bind(&id)
            .bind(name)
            .bind(&creator_email)
            .fetch_one(pool)
            .await
        }
    })
    .await?;

    let created_by = org.created_by.clone();
    add_member_unchecked(db, &org.id, &created_by, roles::ADMIN).await?;
    Ok(org)
}

/// The orgs `email` belongs to, each with the caller's own roles in it.
pub async fn list_orgs_for_user(db: &Db, email: &str) -> Result<Vec<OrgSummary>, RbacError> {
    let pool = db.pool().await?;
    let rows: Vec<OrgRoleRow> = sqlx::query_as(
        "SELECT o.id, o.name, r.role
         FROM org_roles r
         JOIN organizations o ON o.id = r.org_id
         WHERE r.user_email = $1
         ORDER BY o.id, r.role",
    )
    .bind(normalize_email(email))
    .fetch_all(pool)
    .await?;

    let mut summaries: Vec<OrgSummary> = Vec::new();
    for row in rows {
        match summaries.last_mut() {
            Some(last) if last.id == row.id => last.roles.push(row.role),
            _ => summaries.push(OrgSummary {
                id: row.id,
                name: row.name,
                roles: vec![row.role],
            }),
        }
    }
    Ok(summaries)
}

/// The full roster of `org_id`: `RbacError::OrgNotFound` if the org itself
/// does not exist.
pub async fn list_members(db: &Db, org_id: &str) -> Result<Vec<MemberSummary>, RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }

    let pool = db.pool().await?;
    let rows: Vec<MemberRoleRow> = sqlx::query_as(
        "SELECT user_email AS email, role FROM org_roles WHERE org_id = $1 ORDER BY user_email, role",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;

    let mut members: Vec<MemberSummary> = Vec::new();
    for row in rows {
        match members.last_mut() {
            Some(last) if last.email == row.email => last.roles.push(row.role),
            _ => members.push(MemberSummary {
                email: row.email,
                roles: vec![row.role],
            }),
        }
    }
    Ok(members)
}

/// Grant `role` to `email` in `org_id`, provisioning the `users` row first if
/// this is a new email — this is "invite" and "add user", the same
/// mechanism. Not best-effort like `auth::request_key`'s call to
/// `users::record`: an invite that silently fails to provision the user
/// would look like it worked and then never let them in.
async fn add_member_unchecked(
    db: &Db,
    org_id: &str,
    email: &str,
    role: &str,
) -> Result<(), RbacError> {
    let email = normalize_email(email);
    users::record(db, &email).await?;
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let email = email.clone();
        async move {
            sqlx::query(
                "INSERT INTO org_roles (org_id, user_email, role) VALUES ($1, $2, $3)
                 ON CONFLICT (org_id, user_email, role) DO NOTHING",
            )
            .bind(org_id)
            .bind(&email)
            .bind(role)
            .execute(pool)
            .await
            .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Drop `email`'s bare [`roles::DEFAULT`] row in `org_id`, if it has one --
/// the "replace the empty cell" half of granting a real role to someone who
/// so far only had the hidden default. A no-op for a brand new member (no
/// row to drop) and for [`replace_roles`] (which deletes every row for the
/// member up front anyway).
async fn drop_default_role(db: &Db, org_id: &str, email: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_roles WHERE org_id = $1 AND user_email = $2 AND role = $3")
            .bind(org_id)
            .bind(email)
            .bind(roles::DEFAULT)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

/// [`add_member_unchecked`], but for the `POST /private/orgs/{id}/members`
/// route: checks the org exists and that every explicit role is one this
/// project allows, then folds in [`roles::DEFAULT`] (see
/// [`AllowedRoles::with_default_role`]). `roles` itself may be empty: "add
/// this email with no special permission" is a normal request, not an
/// error, and the default role fills the row. If `roles` is *not* empty and
/// `email` already belongs to the org on nothing but the default row (e.g.
/// they were added bare and are now being granted a real role), that row is
/// replaced rather than left sitting alongside the new one. One user can
/// hold several roles in the same org, so this grants all of them at once
/// rather than forcing one call per role.
pub async fn add_member(
    db: &Db,
    allowed: &AllowedRoles,
    org_id: &str,
    email: &str,
    roles: &[String],
) -> Result<(), RbacError> {
    for role in roles {
        if !allowed.contains(role) {
            return Err(RbacError::UnknownRole);
        }
    }
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let email = normalize_email(email);
    if !roles.is_empty() {
        drop_default_role(db, org_id, &email).await?;
    }
    let wanted = allowed.with_default_role(roles);
    for role in &wanted {
        add_member_unchecked(db, org_id, &email, role).await?;
    }
    Ok(())
}

/// Replace `email`'s *visible* role set in `org_id` with exactly `new_roles`
/// — "change user role". `new_roles` may be empty: that revokes every
/// explicit role but still leaves the member with [`roles::DEFAULT`] (see
/// [`AllowedRoles::with_default_role`]), same as an empty `roles` on
/// [`add_member`] — a project's UI need never mention the default either
/// way. It does *not* remove the member; [`leave_org`] is what does that.
/// Refuses if the change would leave the org with no admin at all.
pub async fn replace_roles(
    db: &Db,
    allowed: &AllowedRoles,
    org_id: &str,
    email: &str,
    new_roles: &[String],
) -> Result<(), RbacError> {
    let email = normalize_email(email);
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    if require_member(db, &email, org_id).await.is_err() {
        return Err(RbacError::MemberNotFound);
    }
    for role in new_roles {
        if !allowed.contains(role) {
            return Err(RbacError::UnknownRole);
        }
    }
    let wanted = allowed.with_default_role(new_roles);
    if !wanted.iter().any(|r| r.as_str() == roles::ADMIN)
        && is_last_admin(db, org_id, &email).await?
    {
        return Err(RbacError::LastAdmin);
    }

    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let email = email.clone();
        async move {
            sqlx::query("DELETE FROM org_roles WHERE org_id = $1 AND user_email = $2")
                .bind(org_id)
                .bind(&email)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;

    for role in &wanted {
        add_member_unchecked(db, org_id, &email, role).await?;
    }
    Ok(())
}

/// Remove all of `email`'s own roles in `org_id` — self-service. Refuses if
/// `email` is the org's last admin.
pub async fn leave_org(db: &Db, org_id: &str, email: &str) -> Result<(), RbacError> {
    let email = normalize_email(email);
    if is_last_admin(db, org_id, &email).await? {
        return Err(RbacError::LastAdmin);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| {
        let email = email.clone();
        async move {
            sqlx::query("DELETE FROM org_roles WHERE org_id = $1 AND user_email = $2")
                .bind(org_id)
                .bind(&email)
                .execute(pool)
                .await
                .map(|_| ())
        }
    })
    .await?;
    Ok(())
}

/// Delete an org: its `org_roles` rows first (DSQL has no cascading
/// deletes), then the `organizations` row itself — the same non-transactional
/// sequencing `Meeting::record` uses for its own foreign key.
pub async fn delete_org(db: &Db, org_id: &str) -> Result<(), RbacError> {
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    let pool = db.pool().await?;
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM org_roles WHERE org_id = $1")
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

// ---- invites ----

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

/// One row of `org_invites`.
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

/// A pending (or already-settled) invitation into an org, as the API sees
/// it -- `roles` split back into a list, same shape [`add_member`] takes.
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

/// One entry in `GET /private/invites` -- the org's name folded in, since
/// that route is keyed by the invited email, not the org, and has nothing
/// else to join against for it.
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

async fn get_org_invite(db: &Db, invite_id: &str) -> Result<Option<OrgInvite>, RbacError> {
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

/// Create a pending invite for `email` into `org_id`. Reuses the existing
/// pending row for the same `(org_id, email)` if there is one -- inviting
/// someone a second time before they've answered the first just refreshes
/// its roles and timestamp rather than piling up duplicates. Does **not**
/// touch `org_roles`; that only happens on [`accept_invite`], which is the
/// entire point of an invite over [`add_member`].
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

/// The org's still-pending invites -- `GET /private/orgs/{org_id}/invites`.
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

/// Every org invite still pending for `email` -- the org half of
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

/// Accept a pending invite: mark it `accepted`, then [`add_member`] with
/// its roles (which is also what provisions the `users` row if `email` is
/// new). Returns the settled invite plus the org, so the accept route can
/// hand back both without a second fetch.
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

/// Revoke a pending invite -- the inviting side's cancel. `org_id` is
/// checked so a `team-management` holder in one org can't revoke another
/// org's invite by id.
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

/// Decline a pending invite -- the invited side's "no thanks", for someone
/// who's already logged in and sees it in their pending-invites list rather
/// than following the magic link. `email` must match the invite's, checked
/// here rather than left to the route: nothing else about this call proves
/// who's asking.
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

/// Email the invite (worded differently depending on whether `invite.email`
/// is already a `users` row -- see `users::exists`) and, if that email
/// already has a live session open, push a toast to it right now. Best
/// effort on both: the invite row itself is already committed by the time
/// this runs, same as `auth::request_key`'s best-effort write to `users`.
async fn notify_invite(
    db: &Db,
    messenger: &Messenger,
    management: &ApiGatewayManagement,
    org_name: &str,
    invite: &OrgInvite,
) {
    let existing_user = users::exists(db, &invite.email).await.unwrap_or(false);
    let accept_url = format!(
        "{}/?invite=org:{}",
        crate::core::frontend_base_url(),
        invite.id
    );
    let email_body = json!({
        "template": if existing_user { "org_invite_existing_user" } else { "org_invite_new_user" },
        "org": org_name,
        "accept_url": accept_url,
    });
    messenger.email().send(&invite.email, &email_body).await;

    let toast = json!({
        "type": "invite",
        "kind": "org",
        "invite_id": invite.id,
        "org_id": invite.org_id,
        "org": org_name,
        "message": format!("You've been invited to {org_name}"),
    });
    messenger
        .push_toast(db, management, &invite.email, &toast)
        .await;
}

// ---- routes ----

#[derive(Debug, Deserialize)]
pub struct CreateOrgBody {
    name: String,
    #[serde(default)]
    invites: Vec<InviteEntry>,
}

#[derive(Debug, Deserialize)]
struct InviteEntry {
    email: String,
    roles: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberBody {
    email: String,
    roles: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceRolesBody {
    roles: Vec<String>,
}

/// `POST /private/orgs` — create an org, optionally inviting a batch of
/// emails at once (pending invites, same as
/// `POST /private/orgs/{org_id}/invites` -- see [`create_invite`]). No role
/// check: the caller becomes its admin by creating it.
pub async fn create_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(allowed): Extension<AllowedRoles>,
    Extension(messenger): Extension<Messenger>,
    Extension(management): Extension<ApiGatewayManagement>,
    Json(body): Json<CreateOrgBody>,
) -> Result<Json<Value>, RbacError> {
    // Validated up front, so a bad role in one invite never leaves the org
    // created with only some of the others already added.
    for invite in &body.invites {
        for role in &invite.roles {
            if !allowed.contains(role) {
                return Err(RbacError::UnknownRole);
            }
        }
    }

    let org = create_org(&db, &body.name, &claims.sub).await?;
    for invite in &body.invites {
        let created =
            create_invite(&db, &org.id, &invite.email, &invite.roles, &claims.sub).await?;
        notify_invite(&db, &messenger, &management, &org.name, &created).await;
    }

    Ok(Json(json!({ "org": org })))
}

/// `GET /private/orgs` — the orgs the caller belongs to.
pub async fn list_orgs_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, RbacError> {
    let orgs = list_orgs_for_user(&db, &claims.sub).await?;
    Ok(Json(json!({ "orgs": orgs })))
}

/// `GET /private/orgs/{org_id}` — the roster, members-only.
pub async fn org_detail_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    require_member(&db, &claims.sub, &org_id).await?;
    let members = list_members(&db, &org_id).await?;
    Ok(Json(json!({ "id": org_id, "members": members })))
}

/// `POST /private/orgs/{org_id}/members` — invite/add a member. Requires
/// `team-management`.
pub async fn add_member_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(allowed): Extension<AllowedRoles>,
    Path(org_id): Path<String>,
    Json(body): Json<AddMemberBody>,
) -> Result<Json<Value>, RbacError> {
    require_role(&db, &claims.sub, &org_id, roles::TEAM_MANAGEMENT).await?;
    add_member(&db, &allowed, &org_id, &body.email, &body.roles).await?;
    Ok(Json(
        json!({ "org_id": org_id, "email": body.email, "roles": body.roles }),
    ))
}

/// `PUT /private/orgs/{org_id}/members/{email}/roles` — replace a member's
/// role set. Requires `team-management`.
pub async fn replace_roles_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(allowed): Extension<AllowedRoles>,
    Path((org_id, email)): Path<(String, String)>,
    Json(body): Json<ReplaceRolesBody>,
) -> Result<Json<Value>, RbacError> {
    require_role(&db, &claims.sub, &org_id, roles::TEAM_MANAGEMENT).await?;
    replace_roles(&db, &allowed, &org_id, &email, &body.roles).await?;
    Ok(Json(
        json!({ "org_id": org_id, "email": email, "roles": body.roles }),
    ))
}

/// `DELETE /private/orgs/{org_id}/members/me` — leave. No role required
/// beyond being the caller.
pub async fn leave_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    leave_org(&db, &org_id, &claims.sub).await?;
    Ok(Json(json!({ "org_id": org_id, "left_by": claims.sub })))
}

/// `DELETE /private/orgs/{org_id}` — delete the org. Requires `admin`.
pub async fn delete_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    require_role(&db, &claims.sub, &org_id, roles::ADMIN).await?;
    delete_org(&db, &org_id).await?;
    Ok(Json(json!({ "deleted": org_id })))
}

#[derive(Debug, Deserialize)]
pub struct CreateInviteBody {
    email: String,
    #[serde(default)]
    roles: Vec<String>,
}

/// `POST /private/orgs/{org_id}/invites` — invite a member: a pending row,
/// not instant membership (see [`create_invite`]). Requires
/// `team-management`.
pub async fn create_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(allowed): Extension<AllowedRoles>,
    Extension(messenger): Extension<Messenger>,
    Extension(management): Extension<ApiGatewayManagement>,
    Path(org_id): Path<String>,
    Json(body): Json<CreateInviteBody>,
) -> Result<Json<Value>, RbacError> {
    require_role(&db, &claims.sub, &org_id, roles::TEAM_MANAGEMENT).await?;
    for role in &body.roles {
        if !allowed.contains(role) {
            return Err(RbacError::UnknownRole);
        }
    }
    let org = get_org(&db, &org_id).await?.ok_or(RbacError::OrgNotFound)?;
    let invite = create_invite(&db, &org_id, &body.email, &body.roles, &claims.sub).await?;
    notify_invite(&db, &messenger, &management, &org.name, &invite).await;
    Ok(Json(json!({ "invite": invite })))
}

/// `GET /private/orgs/{org_id}/invites` — the org's still-pending invites.
/// Requires `team-management`.
pub async fn list_invites_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    require_role(&db, &claims.sub, &org_id, roles::TEAM_MANAGEMENT).await?;
    let invites = list_invites(&db, &org_id).await?;
    Ok(Json(json!({ "invites": invites })))
}

/// `DELETE /private/orgs/{org_id}/invites/{invite_id}` — revoke a pending
/// invite. Requires `team-management`.
pub async fn revoke_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path((org_id, invite_id)): Path<(String, String)>,
) -> Result<Json<Value>, RbacError> {
    require_role(&db, &claims.sub, &org_id, roles::TEAM_MANAGEMENT).await?;
    revoke_invite(&db, &org_id, &invite_id).await?;
    Ok(Json(json!({ "revoked": invite_id })))
}

/// `POST /invites/org/{invite_id}/accept` — the magic link: **public**, no
/// bearer token (same trust model as `/auth/request-key/{email}` -- this
/// module already has no proof of email ownership). Accepts the invite and
/// signs the invited email a token in the same call, shaped enough like
/// `request_key`'s response that the frontend can adopt it the same way.
pub async fn accept_invite_route(
    State(auth): State<AuthState>,
    Extension(db): Extension<Db>,
    Extension(allowed): Extension<AllowedRoles>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    let (invite, org) = accept_invite(&db, &allowed, &invite_id).await?;
    let (token, expires_at) = auth.issue(&invite.email).await?;
    Ok(Json(json!({
        "token": token,
        "token_type": "Bearer",
        "email": invite.email,
        "expires_at": expires_at,
        "org": org,
    })))
}

/// `POST /private/invites/org/{invite_id}/decline` — the invited side's "no
/// thanks", for someone who's already logged in. Bearer required; the
/// email-match check is [`decline_invite`]'s.
pub async fn decline_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    decline_invite(&db, &invite_id, &claims.sub).await?;
    Ok(Json(json!({ "declined": invite_id })))
}
