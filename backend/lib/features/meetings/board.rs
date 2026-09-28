//! Freehand drawing on top of a shared screen: strokes are relayed only,
//! nothing persisted, same shape as [`super::chat`] -- see that module's
//! doc. Two extra message shapes exist purely for a late joiner's
//! catch-up: rather than the backend holding any state, a joiner asks the
//! tile's owner directly and the owner's own client answers with whatever
//! it currently has drawn, over a targeted send -- same shape as
//! [`super::FORCE_CHANNEL`], `messenger.send(CHANNEL, &[target], ...)`.
//!
//! A meeting can have two simultaneous screen shares, so every message
//! carries `owner_id` -- the presenting user's id -- to say which tile's
//! board it belongs to. A Chime tile id would not do: it is local to each
//! client's own session, not an identity shared across clients.

use serde_json::json;

use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;

pub const CHANNEL: &str = "meeting_board";

/// Wire the channel up. Meant to be called once, at startup.
///
/// Trusts `from` (the socket's own authenticated user) but not
/// `message.meeting_id` on its own -- same rule as [`super::chat::register`]:
/// a sender who isn't currently an attendee is silently dropped rather than
/// broadcast or forwarded. No check that `from` is actually a tile's owner
/// beyond attendee membership -- a meeting has no roles to gate this by,
/// only "are you in it", and the worst case is a wrong/empty board for one
/// drawing feature, not something worth a stricter check.
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(CHANNEL, move |messenger, from, message| {
        let db = db.clone();
        async move {
            let Some(meeting_id) = message.get("meeting_id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(msg_type) = message.get("type").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(owner_id) = message.get("owner_id").and_then(|v| v.as_str()) else {
                return;
            };

            let attendees = match super::attendee_ids(&db, meeting_id).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!("meeting_board: could not look up attendees: {err:?}");
                    return;
                }
            };
            if !attendees.contains(&from) {
                return;
            }

            match msg_type {
                "add" => {
                    let Some(stroke) = message.get("stroke") else {
                        return;
                    };
                    let others: Vec<String> =
                        attendees.into_iter().filter(|id| id != &from).collect();
                    if others.is_empty() {
                        return;
                    }
                    messenger
                        .send(
                            CHANNEL,
                            &others,
                            json!({
                                "type": "add",
                                "meeting_id": meeting_id,
                                "owner_id": owner_id,
                                "stroke": stroke,
                            }),
                        )
                        .await;
                }
                "erase" => {
                    let Some(stroke_id) = message.get("stroke_id").and_then(|v| v.as_str()) else {
                        return;
                    };
                    let others: Vec<String> =
                        attendees.into_iter().filter(|id| id != &from).collect();
                    if others.is_empty() {
                        return;
                    }
                    messenger
                        .send(
                            CHANNEL,
                            &others,
                            json!({
                                "type": "erase",
                                "meeting_id": meeting_id,
                                "owner_id": owner_id,
                                "stroke_id": stroke_id,
                            }),
                        )
                        .await;
                }
                "request_state" => {
                    if !attendees.contains(&owner_id.to_string()) {
                        return;
                    }
                    messenger
                        .send(
                            CHANNEL,
                            &[owner_id.to_string()],
                            json!({
                                "type": "request_state",
                                "meeting_id": meeting_id,
                                "owner_id": owner_id,
                                "for": from,
                            }),
                        )
                        .await;
                }
                "state" => {
                    let Some(to) = message.get("to").and_then(|v| v.as_str()) else {
                        return;
                    };
                    let Some(strokes) = message.get("strokes") else {
                        return;
                    };
                    if !attendees.contains(&to.to_string()) {
                        return;
                    }
                    messenger
                        .send(
                            CHANNEL,
                            &[to.to_string()],
                            json!({
                                "type": "state",
                                "meeting_id": meeting_id,
                                "owner_id": owner_id,
                                "to": to,
                                "strokes": strokes,
                            }),
                        )
                        .await;
                }
                _ => {}
            }
        }
    });
}
