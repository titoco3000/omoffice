//! Waving at someone: a nudge to come talk, with no meeting behind it yet.
//! Unlike [`super::meeting_invites`] nothing is created or stored -- the
//! waved-at user's tab is what starts or joins a meeting, and only once
//! they answer the wave (by joining the waver, the same as clicking them in
//! the roster). Relay only, same shape as [`super::access_requests`].
//!
//! The waver sends `{action: "wave", org_id, to}`; it's relayed to `to` as
//! `{action: "wave", org_id, from}` and acknowledged back to the waver as
//! `{action: "sent", org_id, to}` -- only if `to` has an open socket, so the
//! waver's "waved" mark means someone actually got it.

use serde_json::json;

use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;
use crate::core::rbac;

pub const CHANNEL: &str = "meeting_wave";

/// Wire the channel up. Meant to be called once, at startup.
///
/// Trusts `from` (the socket's own authenticated user) but not the
/// message: `from` and `to` must both be members of `org_id`, and not the
/// same user. Silently dropped otherwise, same as
/// [`super::access_requests`].
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(CHANNEL, move |messenger, from, message| {
        let db = db.clone();
        async move {
            let Some(action) = message.get("action").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(org_id) = message.get("org_id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(to) = message.get("to").and_then(|v| v.as_str()) else {
                return;
            };
            if to == from || action != "wave" {
                return;
            }

            for user_id in [from.as_str(), to] {
                if let Err(err) = rbac::require_member(&db, org_id, user_id).await {
                    if !matches!(err, rbac::RbacError::Forbidden) {
                        tracing::warn!("meeting_wave: could not check membership: {err:?}");
                    }
                    return;
                }
            }

            let to = [to.to_string()];
            if !messenger.is_connected(&to[0]).await {
                return;
            }
            messenger
                .send(
                    CHANNEL,
                    &to,
                    json!({ "action": "wave", "org_id": org_id, "from": from }),
                )
                .await;
            messenger
                .send(
                    CHANNEL,
                    std::slice::from_ref(&from),
                    json!({ "action": "sent", "org_id": org_id, "to": to[0] }),
                )
                .await;
        }
    });
}
