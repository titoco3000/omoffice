//! Remote mute / camera-off: any attendee may force another attendee's mic
//! muted or camera off -- not gated to the meeting's creator, unlike
//! [`super::force_join`], since this only ever turns something off, and
//! only between two people already in the same meeting.
//!
//! Enforced by relay only, same shape as [`super::chat`]: the backend
//! checks both `from` and the target are current attendees of
//! `meeting_id`, then pushes the instruction to the target's own socket
//! alone. That tab is what actually calls `chime.setMicMuted`/`setCameraOn`
//! -- once it does, every other attendee already sees the effect through
//! Chime's own live audio/video state, so nothing needs broadcasting to
//! anyone else. There is no way to force someone *on* -- only the target's
//! own tab may turn its mic or camera back on.

use serde_json::json;

use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;

pub const CHANNEL: &str = "meeting_control";

/// Wire the channel up. Meant to be called once, at startup.
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(CHANNEL, move |messenger, from, message| {
        let db = db.clone();
        async move {
            let Some(meeting_id) = message.get("meeting_id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(target) = message.get("target_user_id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(action) = message.get("action").and_then(|v| v.as_str()) else {
                return;
            };
            if !matches!(action, "mute" | "camera_off") || target == from {
                return;
            }

            let attendees = match super::attendee_ids(&db, meeting_id).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!("meeting_control: could not look up attendees: {err:?}");
                    return;
                }
            };
            if !attendees.iter().any(|id| id == &from) || !attendees.iter().any(|id| id == target) {
                return;
            }

            messenger
                .send(
                    CHANNEL,
                    &[target.to_string()],
                    json!({ "meeting_id": meeting_id, "action": action }),
                )
                .await;
        }
    });
}
