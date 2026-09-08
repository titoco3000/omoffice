//! Where a person currently is: which org they are looking at, which
//! meeting they are in. The browser reports it on the `presence` channel
//! whenever it changes; this module is the check between that claim and
//! `active_users`, so a socket can only be added to a room its owner is
//! allowed into.
//!
//! It lives beside `chat` rather than inside `core/messenger/` on purpose:
//! deciding who may join an org's room is RBAC's business, and
//! `core/messenger/` knows nothing about orgs or meetings beyond the two
//! nullable columns it stores.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path};
use axum::Json;
use serde_json::{json, Value};

use crate::auth::Claims;
use crate::core::db::Db;
use crate::core::messenger::{active_users, Messenger, Origin};
use crate::meetings::Meeting;
use crate::rbac::{self, RbacError};

/// The channel the browser reports on; `frontend/src/messenger.js` sends
/// the whole context under this name.
pub const CHANNEL: &str = "presence";

/// Wire the channel up. Called once, from `main::app`.
pub fn register(messenger: &Messenger) {
    messenger.on_message(CHANNEL, |messenger, message, origin| async move {
        let org = message.get("org").and_then(Value::as_str);
        let meeting = message.get("meeting").and_then(Value::as_str);

        // A browser re-reports the whole context on every change, so most
        // frames say what the row already says. Skipping those keeps a tab
        // switch from writing to the database.
        if origin.org.as_deref() == org && origin.meeting.as_deref() == meeting {
            return;
        }
        if !allowed(&messenger, &origin, org, meeting).await {
            return;
        }

        // A report is the whole context, so a claim that fails a check is
        // dropped whole rather than applied by halves.
        if let Err(err) = messenger
            .set_context(&origin.connection_id, org, meeting)
            .await
        {
            tracing::warn!("presence: could not store a context report: {err:?}");
        }
    });
}

/// Both halves of a reported context, checked before any of it is stored.
async fn allowed(
    messenger: &Messenger,
    origin: &Origin,
    org: Option<&str>,
    meeting: Option<&str>,
) -> bool {
    if let Some(org) = org {
        if rbac::require_member(messenger.db(), &origin.user, org)
            .await
            .is_err()
        {
            return false;
        }
    }
    if let Some(code) = meeting {
        // Only that the meeting is real. A stronger check would read
        // `meeting_attendees`, but that row is written best effort (see
        // `meetings::record_attendee`), so its absence would lock real
        // participants out of a meeting-wide message.
        match Meeting::get(messenger.db(), code).await {
            Ok(Some(_)) => {}
            _ => return false,
        }
    }
    true
}

/// `GET /private/orgs/{org_id}/presence` — who has a socket open reporting
/// this org, and which meeting each of them is in. The read side of the
/// same `active_users` rows the handler above writes: the sidebar's roster
/// polls it, so "active" means "has a socket", not "has a membership row".
///
/// Membership-gated like every other org read: a roster is org data.
pub async fn org_presence_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    rbac::require_member(&db, &claims.sub, &org_id).await?;

    // One row per socket, so a person with three tabs open is three rows.
    // Fold them to one entry each, keeping any meeting one of the tabs is in.
    let mut users: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in active_users::in_org(&db, &org_id).await? {
        let entry = users.entry(row.user_email).or_insert(None);
        if entry.is_none() {
            *entry = row.meeting;
        }
    }

    let users: Vec<Value> = users
        .into_iter()
        .map(|(email, meeting)| json!({ "email": email, "meeting": meeting }))
        .collect();

    Ok(Json(json!({ "users": users })))
}
