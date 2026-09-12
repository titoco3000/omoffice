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
//! [`register`] subscribes to the messenger's [`CONNECTION`](web_messenger::CONNECTION)
//! channel so `org_users.online` reflects whether a member currently has a
//! live socket, without the messenger itself knowing what an org is.

pub mod org_invites;
pub use org_invites::*;

use axum::Router;
use chrono::{DateTime, Utc};

use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::{Db, DbError};

/// The roles native to the RBAC. A member with none of these still has a
/// row in `org_users` -- membership and role are separate, see the module doc.
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const BILLING: &str = "billing";
    pub const TEAM_MANAGEMENT: &str = "team-management";
}

#[derive(Debug)]
pub struct Org {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// One row of `org_users`, folded with that member's `org_roles`.
#[derive(Debug)]
pub struct OrgMember {
    pub user_id: String,
    /// This org's own display name for the member, if they set one.
    pub name: Option<String>,
    pub online: bool,
    pub roles: Vec<String>,
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

/// Find or create the `users` row for `email`, returning its id.
pub async fn register_user(db: &Db, email: &str, name: Option<&str>) -> Result<String, RbacError> {
    let _ = (db, email, name);
    todo!("insert into users on conflict (email) do nothing, return its id")
}

/// Create an org. The creator is always its first `admin` -- an org can
/// never exist with zero admins.
pub async fn create_org(db: &Db, name: &str, creator: &str) -> Result<Org, RbacError> {
    let _ = (db, name, creator);
    todo!()
}

/// Delete an org: its `org_roles` and `org_users` rows first (DSQL has no
/// cascading deletes), then the `organizations` row itself.
pub async fn delete_org(db: &Db, org_id: &str) -> Result<(), RbacError> {
    let _ = (db, org_id);
    todo!()
}

pub async fn rename_org(db: &Db, org_id: &str, name: &str) -> Result<(), RbacError> {
    let _ = (db, org_id, name);
    todo!()
}

/// Renames how `user_id` appears inside `org_id` only (`org_users.name`).
pub async fn rename_in_org(
    db: &Db,
    org_id: &str,
    user_id: &str,
    name: &str,
) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id, name);
    todo!()
}

/// Renames `user_id` everywhere (`users.name`).
pub async fn rename_globally(db: &Db, user_id: &str, name: &str) -> Result<(), RbacError> {
    let _ = (db, user_id, name);
    todo!()
}

/// The full roster of `org_id`, each member's roles folded in.
pub async fn list_members(db: &Db, org_id: &str) -> Result<Vec<OrgMember>, RbacError> {
    let _ = (db, org_id);
    todo!()
}

/// Add `user_id` to `org_id` -- a bare membership row, no roles yet. What an
/// accepted invite calls before [`assign_role`] for each invited role.
pub async fn add_member(db: &Db, org_id: &str, user_id: &str) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id);
    todo!()
}

pub async fn assign_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id, role);
    todo!()
}

pub async fn revoke_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id, role);
    todo!()
}

/// Remove `user_id` from `org_id` entirely -- both `org_users` and
/// `org_roles`. Refuses if they are the org's last admin; see the module doc.
pub async fn leave_org(db: &Db, org_id: &str, user_id: &str) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id);
    todo!()
}

/// The generic "does this user hold this role" gate.
pub async fn require_role(
    db: &Db,
    org_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id, role);
    todo!()
}

/// Any membership at all, no particular role -- gates a roster read to
/// members only.
pub async fn require_member(db: &Db, org_id: &str, user_id: &str) -> Result<(), RbacError> {
    let _ = (db, org_id, user_id);
    todo!()
}

/// Wire `org_users.online` to the messenger's connection channel. Meant to
/// be called once, at startup.
pub fn register(messenger: &Messenger) {
    messenger.on_message(
        web_messenger::CONNECTION,
        |_messenger, user_id, message| async move {
            let online = message.as_str() == Some(web_messenger::CONNECTED);
            let _ = (user_id, online);
            todo!("set org_users.online = {online} for every org this user belongs to")
        },
    );
}

/// `/rbac/*` -- org and membership management.
pub fn routes() -> Router {
    todo!("orgs CRUD, members, roles, plus org_invites' accept/decline/revoke")
}
