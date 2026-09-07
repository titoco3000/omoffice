//! The axum handlers for `/private/orgs/...` and the invite accept/decline
//! routes — thin bindings over [`super::orgs`] and [`super::invites`].

use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::invites::{
    accept_invite, create_invite, decline_invite, list_invites, notify_invite, revoke_invite,
};
use super::orgs::{
    add_member, create_org, delete_org, leave_org, list_members, list_orgs_for_user, replace_roles,
};
use super::{get_org, require_role, roles, AllowedRoles, RbacError};
use crate::auth::{AuthState, Claims};
use crate::core::apigw_management::ApiGatewayManagement;
use crate::core::db::Db;
use crate::core::messenger::Messenger;

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
    Extension(messenger): Extension<Messenger>,
    Extension(management): Extension<ApiGatewayManagement>,
    Json(body): Json<CreateOrgBody>,
) -> Result<Json<Value>, RbacError> {
    // Validated up front so a bad role in one invite never leaves the org
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
    super::require_member(&db, &claims.sub, &org_id).await?;
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
/// not instant membership. Requires `team-management`.
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

/// `POST /invites/org/{invite_id}/accept` — the magic link: public, no
/// bearer token, shaped like `request_key`'s response.
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
/// thanks", for someone already logged in.
pub async fn decline_invite_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(invite_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    decline_invite(&db, &invite_id, &claims.sub).await?;
    Ok(Json(json!({ "declined": invite_id })))
}
