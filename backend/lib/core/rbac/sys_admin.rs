//! The secret `sys-admin` role: a platform-wide operator, not scoped to any
//! org -- which is why it lives in its own table
//! ([`SYS_ADMINS`](crate::schema::SYS_ADMINS)) rather than as one more
//! `org_roles` value. No org-facing route ever mentions it; only `/admin/*`
//! (mounted in `router.rs`) and `GET /rbac/me`'s `sys_admin` flag, which is
//! what shows the profile page's admin button.
//!
//! There is no route that creates the very first sys-admin -- seed one by
//! hand (`make db-shell`). Like an org's last `admin`, the last sys-admin
//! can never be removed, so the role can't be orphaned from the UI.

use axum::extract::{Extension, Path};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{normalize_email, register_user, Org, RbacError};
use crate::core::auth::Claims;
use crate::core::data::db::{retry_on_conflict, Db};

pub async fn is_sys_admin(db: &Db, user_id: &str) -> Result<bool, RbacError> {
    let pool = db.pool().await?;
    let row: Option<i32> = sqlx::query_scalar("SELECT 1 FROM sys_admins WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

async fn require_sys_admin(db: &Db, user_id: &str) -> Result<(), RbacError> {
    if is_sys_admin(db, user_id).await? {
        Ok(())
    } else {
        Err(RbacError::Forbidden)
    }
}

/// Membership of `org_id`, or being a sys-admin -- gates the read-only org
/// views (map, positions) the admin panel borrows from other features.
pub async fn require_member_or_sys_admin(
    db: &Db,
    org_id: &str,
    user_id: &str,
) -> Result<(), RbacError> {
    if super::is_member(db, org_id, user_id).await? {
        return Ok(());
    }
    require_sys_admin(db, user_id).await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SysAdmin {
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

pub async fn list_sys_admins(db: &Db) -> Result<Vec<SysAdmin>, RbacError> {
    let pool = db.pool().await?;
    let admins = sqlx::query_as(
        "SELECT s.user_id, u.email, u.name
         FROM sys_admins s
         JOIN users u ON u.id = s.user_id
         ORDER BY u.email",
    )
    .fetch_all(pool)
    .await?;
    Ok(admins)
}

/// Make `email` a sys-admin, creating the user if there is none yet.
pub async fn add_sys_admin(db: &Db, email: &str) -> Result<(), RbacError> {
    let user_id = register_user(db, email, None).await?;
    let pool = db.pool().await?;
    let user_id = user_id.as_str();
    retry_on_conflict(|| async move {
        sqlx::query(
            "INSERT INTO sys_admins (user_id) VALUES ($1)
             ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await?;
    Ok(())
}

pub async fn remove_sys_admin(db: &Db, user_id: &str) -> Result<(), RbacError> {
    let pool = db.pool().await?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT user_id FROM sys_admins")
        .fetch_all(pool)
        .await?;
    if ids.len() == 1 && ids[0] == user_id {
        return Err(RbacError::LastSysAdmin);
    }
    retry_on_conflict(|| async move {
        sqlx::query("DELETE FROM sys_admins WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OrgOverview {
    pub id: String,
    pub name: String,
    pub picture: Option<String>,
    pub user_count: i64,
    pub online_count: i64,
}

pub async fn list_org_overviews(db: &Db) -> Result<Vec<OrgOverview>, RbacError> {
    let pool = db.pool().await?;
    let orgs = sqlx::query_as(
        "SELECT o.id, o.name, o.picture,
                COUNT(u.user_id) AS user_count,
                COUNT(u.user_id) FILTER (WHERE u.online) AS online_count
         FROM organizations o
         LEFT JOIN org_users u ON u.org_id = o.id
         GROUP BY o.id, o.name, o.picture
         ORDER BY o.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(orgs)
}

async fn get_org(db: &Db, org_id: &str) -> Result<Org, RbacError> {
    let pool = db.pool().await?;
    sqlx::query_as::<_, Org>(
        "SELECT id, name, created_by, picture, created_at FROM organizations WHERE id = $1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await?
    .ok_or(RbacError::OrgNotFound)
}

#[derive(Deserialize)]
struct AddBody {
    email: String,
}

async fn list_sys_admins_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, RbacError> {
    require_sys_admin(&db, &claims.sub).await?;
    Ok(Json(json!({ "sys_admins": list_sys_admins(&db).await? })))
}

async fn add_sys_admin_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<AddBody>,
) -> Result<Json<Value>, RbacError> {
    require_sys_admin(&db, &claims.sub).await?;
    if normalize_email(&body.email).is_empty() {
        return Err(RbacError::InvalidEmail);
    }
    add_sys_admin(&db, &body.email).await?;
    Ok(Json(json!({ "sys_admins": list_sys_admins(&db).await? })))
}

async fn remove_sys_admin_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    require_sys_admin(&db, &claims.sub).await?;
    remove_sys_admin(&db, &user_id).await?;
    Ok(Json(json!({ "sys_admins": list_sys_admins(&db).await? })))
}

async fn list_orgs_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, RbacError> {
    require_sys_admin(&db, &claims.sub).await?;
    Ok(Json(json!({ "orgs": list_org_overviews(&db).await? })))
}

/// `GET /admin/org/{org_id}` -- the org itself. Its roster and every org
/// action are the regular `/rbac/org/{org_id}/*` routes, which let a
/// sys-admin through (see [`require_role`](super::require_role)).
async fn org_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    require_sys_admin(&db, &claims.sub).await?;
    Ok(Json(json!({ "org": get_org(&db, &org_id).await? })))
}

pub fn routes() -> Router {
    Router::new()
        .route(
            "/sys-admins",
            get(list_sys_admins_route).post(add_sys_admin_route),
        )
        .route("/sys-admins/{user_id}", delete(remove_sys_admin_route))
        .route("/orgs", get(list_orgs_route))
        .route("/org/{org_id}", get(org_route))
}
