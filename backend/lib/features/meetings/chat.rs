//! In-meeting chat: a channel scoped to one meeting, broadcast to its
//! current attendees. A separate concept from any org-wide chat -- a
//! meeting has no roles of its own to gate a message by, only "are you
//! actually in it".

use crate::core::communication::web_messenger::Messenger;
use crate::core::data::db::Db;

pub const CHANNEL: &str = "meeting_chat";

/// Wire the channel up. Meant to be called once, at startup.
pub fn register(messenger: &Messenger, db: Db) {
    messenger.on_message(CHANNEL, move |messenger, from, message| {
        let db = db.clone();
        async move {
            let _ = (db, messenger, from, message);
            todo!(
                "read message.meeting_id, look up meeting_attendees for it, \
                 then messenger.send(CHANNEL, &attendees, {{\"from\": from, \"text\": ...}})"
            )
        }
    });
}
