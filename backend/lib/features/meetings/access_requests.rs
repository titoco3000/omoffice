//! Requesting access to a locked meeting: a non-attendee asks, current
//! attendees are told -- there is no accept step behind this, unlike
//! [`super::meeting_invites`]. Whoever's already in the meeting answers by
//! doing what they'd do anyway to let someone in -- sending an invite or
//! forcing them straight in -- so this is relay only, same shape as
//! [`super::chat`] and [`super::controls`], not a tracked resource of its
//! own.

use serde_json::json;

use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;

pub const CHANNEL: &str = "meeting_access_request";

/// Wire the channel up. Meant to be called once, at startup.
///
/// Trusts `from` (the socket's own authenticated user) but not
/// `message.meeting_id` on its own -- the meeting has to actually exist, and
/// `from` must not already be one of its attendees (nothing to request
/// access to, and it would otherwise leak a request notification to the
/// requester's own tab). Silently dropped otherwise, same as
/// [`super::chat`]'s own attendee check.
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(CHANNEL, move |messenger, from, message| {
        let db = db.clone();
        async move {
            let Some(meeting_id) = message.get("meeting_id").and_then(|v| v.as_str()) else {
                return;
            };

            let attendees = match super::attendee_ids(&db, meeting_id).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!("meeting_access_request: could not look up attendees: {err:?}");
                    return;
                }
            };
            if attendees.is_empty() || attendees.contains(&from) {
                return;
            }

            messenger
                .send(
                    CHANNEL,
                    &attendees,
                    json!({ "meeting_id": meeting_id, "from": from }),
                )
                .await;
        }
    });
}
