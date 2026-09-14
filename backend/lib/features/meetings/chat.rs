//! In-meeting chat: a channel scoped to one meeting, broadcast to its
//! current attendees. A separate concept from any org-wide chat -- a
//! meeting has no roles of its own to gate a message by, only "are you
//! actually in it". Nothing is persisted: a message exists only as long as
//! it's in flight between sockets, same as [`super::ROOM_CHANNEL`] pushes --
//! there is no `meeting_chat` table, and a client that joins mid-meeting
//! sees nothing sent before it connected.

use serde_json::json;

use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;

pub const CHANNEL: &str = "meeting_chat";

/// Wire the channel up. Meant to be called once, at startup.
///
/// Trusts `from` (the socket's own authenticated user, not anything the
/// client claims) but not `message.meeting_id` on its own -- a sender who
/// isn't currently among that meeting's attendees is silently dropped
/// rather than broadcast, so one tab can't push a message into a meeting
/// it never joined just by guessing an id.
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(CHANNEL, move |messenger, from, message| {
        let db = db.clone();
        async move {
            let Some(meeting_id) = message.get("meeting_id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(text) = message.get("text").and_then(|v| v.as_str()) else {
                return;
            };
            let text = text.trim();
            if text.is_empty() {
                return;
            }

            let attendees = match super::attendee_ids(&db, meeting_id).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!("meeting_chat: could not look up attendees: {err:?}");
                    return;
                }
            };
            if !attendees.contains(&from) {
                return;
            }

            let others: Vec<String> = attendees.into_iter().filter(|id| id != &from).collect();
            if others.is_empty() {
                return;
            }

            messenger
                .send(
                    CHANNEL,
                    &others,
                    json!({ "meeting_id": meeting_id, "from": from, "text": text }),
                )
                .await;
        }
    });
}
