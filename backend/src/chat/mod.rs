//! Chat: a per-org room with no history. It owns one channel name and the
//! rule "a message goes to the sender's org" — nothing else. Delivery,
//! sockets and rosters all belong to [`Messenger`], which is why this
//! module has no routes and no queries of its own.

use serde_json::{json, Value};

use crate::core::messenger::{Messenger, Via};

/// The channel both halves agree on; `frontend/src/messenger.js` sends and
/// subscribes under the same name.
pub const CHANNEL: &str = "chat";

/// Wire the channel up. Called once, from `main::app`.
pub fn register(messenger: &Messenger) {
    messenger.on_message(CHANNEL, |messenger, message, origin| async move {
        let Some(text) = message.get("text").and_then(Value::as_str) else {
            return;
        };
        // No org reported yet (a socket that has not sent `presence`, or a
        // person looking at no org) means no room to speak into.
        let Some(org) = origin.org.clone() else {
            return;
        };

        // No membership check here: a socket only carries an org after
        // `presence` checked it (see `presence/mod.rs`), and the roster this
        // fans out to is that same set of checked sockets.
        //
        // The sender's own sockets are included, so every tab they have open
        // shows the message and the browser never has to echo it locally.
        messenger
            .send_to_users_on_org(
                &json!({ "from": origin.user, "text": text }),
                CHANNEL,
                &[Via::Websocket],
                &org,
            )
            .await;
    });
}
