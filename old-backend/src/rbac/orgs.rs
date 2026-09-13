//! Org creation and membership: who belongs to an org, with which roles.

use serde::Serialize;
use uuid::Uuid;

use super::{
    is_last_admin, normalize_email, org_exists, require_member, roles, AllowedRoles, Org, RbacError,
};
use crate::core::db::{retry_on_conflict, Db};
use crate::users;

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

/// Grant `role` to `email` in `org_id`, provisioning the `users` row first.
/// Not best-effort: a silent failure would look like it worked.
pub(super) async fn add_member_unchecked(
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

/// Drop `email`'s bare [`roles::DEFAULT`] row in `org_id`, if it has one.
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

/// [`add_member_unchecked`] for the route: checks the org and roles are
/// valid, then folds in [`roles::DEFAULT`] for an empty list.
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

/// Replace `email`'s role set with `new_roles`; refuses if that would leave
/// the org with no admin. [`leave_org`] is what removes a member.
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
/// deletes), then the `organizations` row itself.
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
