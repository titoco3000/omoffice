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
//!
//! Presence is pushed, not polled: a join, a leave or a disconnect
//! recomputes the affected org's snapshot and fans it straight out on
//! `org_presence` to whoever is currently looking at that org (the same
//! audience `send_to_users_on_org` always means -- people whose own
//! reported context is that org). `GET /private/orgs/{id}/presence` still
//! exists for the one read a tab has no push to answer yet: its own first
//! paint, before anything has changed.
//!
//! A meeting gets the same viewer-count treatment on `meeting_presence`.
//! Every recompute also hands the fresh count to
//! [`meetings::pool::note_occupancy`], which is how `pool` learns a meeting
//! just went (or stopped being) a lone viewer, without this module knowing
//! anything about Chime or the pool beyond that one call — the same way
//! this module knows nothing about `core/messenger`'s transports.

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{Extension, Path};
use axum::Json;
use serde_json::{json, Value};

use crate::auth::Claims;
use crate::core::db::{Db, DbError};
use crate::core::messenger::{active_users, Messenger, Origin, Via};
use crate::meetings::{pool, Meeting};
use crate::rbac::{self, RbacError};

/// The channel a browser reports its own context on.
pub const CHANNEL: &str = "presence";

/// The channel a browser listens on for someone else's org changing.
pub const BROADCAST_CHANNEL: &str = "org_presence";

/// The channel a browser listens on for its own meeting's occupancy
/// changing -- someone else joining or leaving.
pub const MEETING_BROADCAST_CHANNEL: &str = "meeting_presence";

/// Wire the channel up. Called once, from `main::app`.
pub fn register(messenger: &Messenger) {
    messenger.on_message(CHANNEL, |messenger, message, origin| async move {
        let org = message.get("org").and_then(Value::as_str);
        let meeting = message.get("meeting").and_then(Value::as_str);

        // A browser re-reports the whole context on every change, so most
        // frames say what the row already says. Skipping those keeps a tab
        // switch from writing to the database -- and from broadcasting
        // nothing changed.
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
            return;
        }

        let mut changed_orgs: Vec<&str> = Vec::new();
        if let Some(left) = origin.org.as_deref() {
            changed_orgs.push(left);
        }
        if let Some(joined) = org {
            if Some(joined) != origin.org.as_deref() {
                changed_orgs.push(joined);
            }
        }
        for org_id in changed_orgs {
            broadcast(&messenger, org_id).await;
        }

        let mut changed_meetings: Vec<&str> = Vec::new();
        if let Some(left) = origin.meeting.as_deref() {
            changed_meetings.push(left);
        }
        if let Some(joined) = meeting {
            if Some(joined) != origin.meeting.as_deref() {
                changed_meetings.push(joined);
            }
        }
        for code in changed_meetings {
            broadcast_meeting(&messenger, code).await;
        }
    });

    messenger.on_disconnect(|messenger, org_id, meeting| async move {
        if let Some(org_id) = org_id {
            broadcast(&messenger, &org_id).await;
        }
        if let Some(code) = meeting {
            broadcast_meeting(&messenger, &code).await;
        }
    });
}

/// Recompute one org's presence and push it to whoever is currently
/// looking at that org.
async fn broadcast(messenger: &Messenger, org_id: &str) {
    let body = match snapshot(messenger.db(), org_id).await {
        Ok(body) => body,
        Err(err) => {
            tracing::warn!("presence: could not read org {org_id} to broadcast it: {err:?}");
            return;
        }
    };
    messenger
        .send_to_users_on_org(&body, BROADCAST_CHANNEL, &[Via::Websocket], org_id)
        .await;
}

/// Recompute one meeting's occupancy, tell `pool` about it (the moment a
/// meeting drops under 2 viewers, or climbs back, is exactly what it needs
/// to know to stamp or clear `idle_since` -- see `meetings/pool.rs`), and
/// push the fresh count to whoever is currently on that meeting's page.
async fn broadcast_meeting(messenger: &Messenger, code: &str) {
    let (body, viewer_count) = match meeting_snapshot(messenger.db(), code).await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            tracing::warn!("presence: could not read meeting {code} to broadcast it: {err:?}");
            return;
        }
    };
    if let Err(err) = pool::note_occupancy(messenger.db(), code, viewer_count).await {
        tracing::warn!("presence: could not update chime idleness for {code}: {err:?}");
    }
    messenger
        .send_to_users_on_meeting(&body, MEETING_BROADCAST_CHANNEL, &[Via::Websocket], code)
        .await;
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

/// Who has a socket open reporting `org_id`, and which meeting each of them
/// is in. One row per socket, so a person with three tabs open is three
/// rows; folded to one entry each, keeping any meeting one of the tabs is
/// in. Shared by the HTTP read route and the websocket broadcast, so a
/// viewer sees the same shape whichever way the update arrived.
async fn snapshot(db: &Db, org_id: &str) -> Result<Value, DbError> {
    let mut users: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in active_users::in_org(db, org_id).await? {
        let entry = users.entry(row.user_email).or_insert(None);
        if entry.is_none() {
            *entry = row.meeting;
        }
    }

    let users: Vec<Value> = users
        .into_iter()
        .map(|(email, meeting)| json!({ "email": email, "meeting": meeting }))
        .collect();

    Ok(json!({ "org_id": org_id, "users": users }))
}

/// How many distinct people are on `code`'s page -- folded per email the
/// same way [`snapshot`] folds an org's, so someone's second tab doesn't
/// count them twice. Returns the broadcast body alongside the bare count,
/// since [`broadcast_meeting`] needs both.
async fn meeting_snapshot(db: &Db, code: &str) -> Result<(Value, usize), DbError> {
    let mut viewers: BTreeSet<String> = BTreeSet::new();
    for row in active_users::in_meeting(db, code).await? {
        viewers.insert(row.user_email);
    }

    let viewer_count = viewers.len();
    Ok((
        json!({ "meeting": code, "viewer_count": viewer_count }),
        viewer_count,
    ))
}

/// `GET /private/orgs/{org_id}/presence` -- a tab's own first read, before
/// it has a live broadcast to react to. Membership-gated like every other
/// org read: a roster is org data.
pub async fn org_presence_route(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    rbac::require_member(&db, &claims.sub, &org_id).await?;
    Ok(Json(snapshot(&db, &org_id).await?))
}
