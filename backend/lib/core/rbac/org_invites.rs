//! Org invitations: a pending row that only becomes membership on accept.
//! Delivery is always an email -- no "is this person online" branch.

use chrono::{DateTime, Utc};

use super::RbacError;
use crate::core::data::db::Db;

#[derive(Debug)]
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

/// Create a pending invite and email it. Reuses an existing pending row for
/// the same `(org_id, email)` rather than piling up duplicates.
pub async fn create_invite(
    db: &Db,
    org_id: &str,
    email: &str,
    roles: &[String],
    invited_by: &str,
) -> Result<OrgInvite, RbacError> {
    let _ = (db, org_id, email, roles, invited_by);
    todo!("insert/update org_invites, then core::communication::email::send_email the magic link")
}

/// Accept a pending invite: mark it accepted, then [`super::add_member`] and
/// [`super::assign_role`] for each of its roles.
pub async fn accept_invite(db: &Db, invite_id: &str) -> Result<OrgInvite, RbacError> {
    let _ = (db, invite_id);
    todo!()
}

pub async fn decline_invite(db: &Db, invite_id: &str) -> Result<(), RbacError> {
    let _ = (db, invite_id);
    todo!()
}

/// `org_id` is checked so a `team-management` holder in one org can't revoke
/// another org's invite by id.
pub async fn revoke_invite(db: &Db, org_id: &str, invite_id: &str) -> Result<(), RbacError> {
    let _ = (db, org_id, invite_id);
    todo!()
}

pub async fn list_invites(db: &Db, org_id: &str) -> Result<Vec<OrgInvite>, RbacError> {
    let _ = (db, org_id);
    todo!()
}
