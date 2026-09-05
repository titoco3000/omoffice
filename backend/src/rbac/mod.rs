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
//! row if the email is new), [`replace_roles`] ("change user role", and
//! "remove a member" is just replacing with an empty set), [`leave_org`]
//! (self-service, blocked for the last admin), [`delete_org`].

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::Claims;
use crate::core::db::{retry_on_conflict, Db, DbError};
use crate::users;

/// The three roles native to the RBAC. Everything else is a plain string a
/// project defines for itself (see [`RolesConfig::project_roles`]) — there is
/// no enum of "all roles" here on purpose, since that list is per-project.
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const BILLING: &str = "billing";
    pub const TEAM_MANAGEMENT: &str = "team-management";
}

/// Builder for the full set of role names a `POST`/`PUT` to `/private/orgs/...`
/// may assign, mirroring [`crate::auth::AuthConfig`]'s shape: `app()` states
/// the project's own roles in one place, this module only enforces them.
///
/// ```ignore
/// RolesConfig::new().project_roles(["post"]).default_role("member").build()
/// ```
pub struct RolesConfig {
    project_roles: Vec<String>,
    default_role: Option<String>,
}

impl RolesConfig {
    pub fn new() -> Self {
        Self {
            project_roles: Vec::new(),
            default_role: None,
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

    /// A role every member is silently given whenever they're given any role
    /// at all — a "this person belongs to the org" tag the RBAC needs
    /// (`org_roles` is the only table that says someone is a member, so a
    /// member with zero rows in it isn't one) but that a project's own UI
    /// has no reason to offer as a pickable permission. Optional: with none
    /// set, adding someone with an empty role list is a `RbacError::NoRoles`
    /// same as always.
    pub fn default_role(mut self, role: impl Into<String>) -> Self {
        self.default_role = Some(role.into());
        self
    }

    pub fn build(self) -> AllowedRoles {
        let mut known = vec![
            roles::ADMIN.to_string(),
            roles::BILLING.to_string(),
            roles::TEAM_MANAGEMENT.to_string(),
        ];
        known.extend(self.project_roles);
        if let Some(default) = &self.default_role {
            if !known.contains(default) {
                known.push(default.clone());
            }
        }
        AllowedRoles {
            known,
            default_role: self.default_role,
        }
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
    default_role: Option<String>,
}

impl AllowedRoles {
    fn contains(&self, role: &str) -> bool {
        self.known.iter().any(|allowed| allowed == role)
    }

    /// `roles` plus the configured default role, if any and not already
    /// present. Does *not* special-case an empty `roles` on its own — a
    /// caller wanting "empty stays empty" (removing a member entirely is
    /// exactly that) checks for that itself before calling this; see
    /// [`replace_roles`].
    fn with_default_role(&self, roles: &[String]) -> Vec<String> {
        let mut wanted = roles.to_vec();
        if let Some(default) = &self.default_role {
            if !wanted.contains(default) {
                wanted.push(default.clone());
            }
        }
        wanted
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
    /// Adding a member with an empty role set -- a member with no role isn't
    /// a member, since `org_roles` is the only table that says so.
    NoRoles,
    Database(DbError),
}

impl From<DbError> for RbacError {
    fn from(err: DbError) -> Self {
        Self::Database(err)
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
            Self::NoRoles => (
                StatusCode::BAD_REQUEST,
                "no_roles",
                "a member needs at least one role".to_string(),
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

/// Any role at all — gates read access to a roster to members only.
async fn require_member(db: &Db, email: &str, org_id: &str) -> Result<(), RbacError> {
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

/// [`add_member_unchecked`], but for the `POST /private/orgs/{id}/members`
/// route: checks the org exists and that every explicit role is one this
/// project allows, folds in the configured default role (if any — see
/// [`RolesConfig::default_role`]), and refuses only if that still leaves
/// nothing to grant. `roles` itself may be empty: "add this email with no
/// special permission" is a normal request, not an error, as long as a
/// default role fills the row. One user can hold several roles in the same
/// org, so this grants all of them at once rather than forcing one call per
/// role.
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
    let wanted = allowed.with_default_role(roles);
    if wanted.is_empty() {
        return Err(RbacError::NoRoles);
    }
    if !org_exists(db, org_id).await? {
        return Err(RbacError::OrgNotFound);
    }
    for role in &wanted {
        add_member_unchecked(db, org_id, email, role).await?;
    }
    Ok(())
}

/// Replace `email`'s *visible* role set in `org_id` with exactly `new_roles`
/// — "change user role", and "remove a member" is just calling this with an
/// empty slice, which is why an empty `new_roles` is handled first and never
/// gets the default role folded back in: it means take this person out
/// entirely, not demote them to bare membership. A non-empty `new_roles`
/// does get the default role folded in (see [`RolesConfig::default_role`]),
/// the same as [`add_member`] — a project's UI need never mention it either
/// way. Refuses if the change would leave the org with no admin at all.
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
    let wanted = if new_roles.is_empty() {
        Vec::new()
    } else {
        allowed.with_default_role(new_roles)
    };
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
/// emails at once. No role check: the caller becomes its admin by creating it.
pub async fn create_org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Extension(allowed): Extension<AllowedRoles>,
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
        add_member(&db, &allowed, &org.id, &invite.email, &invite.roles).await?;
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
