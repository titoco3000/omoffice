//! Messaging, as one interface with two directions.
//!
//! **Out**: [`Messenger::send`] takes a channel name, a JSON message and a
//! list of recipients (user ids); it resolves each id to whichever socket(s)
//! it currently has open. **In**: [`Messenger::on_message`] registers a
//! handler on a channel, called once per frame a client sends on it -- every
//! handler registered on that channel runs, not just the first or the last,
//! so two features can both care about the same channel without one
//! silently displacing the other's handler.
//!
//! Connecting and disconnecting is itself a channel -- [`CONNECTION`] --
//! rather than a separate presence table: anything that cares who is online
//! subscribes to it, without a second thing (a presence table, an idle
//! sweep) to keep in sync with the socket registry underneath. This is the
//! one channel routinely shared by more than one subscriber (`core::rbac`
//! for "online", `features::meetings` for dropping a meeting's attendee row
//! on a hard disconnect) -- see [`on_message`](Messenger::on_message).
//!
//! Nothing above this module knows a socket exists. The wire frame is
//! `{"channel": ..., "message": ...}` in both directions, so one socket
//! carries every channel a client cares about; see [`web_sockets::Hub`] for
//! the registry underneath.

pub mod web_sockets;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Extension, Path};
use axum::response::Response;
use futures_util::future::BoxFuture;
use serde_json::Value;

/// A channel name both the browser and backend agree on, e.g. `"chat"` or
/// `"meeting_chat"`. A feature owns one: it registers a handler for messages
/// coming in on it, and calls [`Messenger::send`] to push messages out.
pub type Channel = &'static str;

/// Not a message a client ever sends -- fired locally when a socket opens or
/// closes, carrying [`CONNECTED`]/[`DISCONNECTED`] as its message. Subscribe
/// to it the same way as any other channel to learn who just came online or
/// went away, without touching [`web_sockets::Hub`] directly.
pub const CONNECTION: Channel = "connection";
pub const CONNECTED: &str = "connected";
pub const DISCONNECTED: &str = "disconnected";

type Handler = Arc<dyn Fn(Messenger, String, Value) -> BoxFuture<'static, ()> + Send + Sync>;

/// The messaging system. Reaches handlers as an axum `Extension`; cheap to
/// clone, same as [`crate::core::data::db::Db`].
#[derive(Clone)]
pub struct Messenger {
    hub: web_sockets::Hub,
    handlers: Arc<RwLock<HashMap<Channel, Vec<Handler>>>>,
}

impl Messenger {
    pub fn new() -> Self {
        Self {
            hub: web_sockets::Hub::default(),
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Send `message` on `channel` to each of `to` (user ids). Best effort:
    /// a recipient with no open socket simply misses it.
    pub async fn send(&self, channel: Channel, to: &[String], message: Value) {
        let frame = serde_json::json!({ "channel": channel, "message": message });
        for user_id in to {
            self.hub.send(user_id, &frame).await;
        }
    }

    /// True if `user_id` has at least one open socket right now.
    pub async fn is_connected(&self, user_id: &str) -> bool {
        self.hub.is_connected(user_id).await
    }

    /// Register `handler` on `channel`. Additive: a second registration on
    /// the same channel runs alongside the first, not instead of it -- see
    /// the module doc on [`CONNECTION`], the one channel more than one
    /// feature actually shares. The handler is handed a fresh clone of the
    /// `Messenger` rather than capturing one, so answering by sending does
    /// not close a reference cycle back into the `Arc` it lives in.
    pub fn on_message<F, Fut>(&self, channel: Channel, handler: F)
    where
        F: Fn(Messenger, String, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler: Handler =
            Arc::new(move |messenger, from, message| Box::pin(handler(messenger, from, message)));
        self.handlers
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .entry(channel)
            .or_default()
            .push(handler);
    }

    /// One incoming frame: run every handler registered on its channel, in
    /// registration order, each with the message plus who sent it. Also
    /// where [`CONNECTION`] itself fires.
    async fn dispatch(&self, from: &str, frame: Value) {
        let Some(channel) = frame.get("channel").and_then(Value::as_str) else {
            return;
        };
        let handlers = self
            .handlers
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(channel)
            .cloned()
            .unwrap_or_default();
        let message = frame.get("message").cloned().unwrap_or(Value::Null);
        for handler in handlers {
            handler(self.clone(), from.to_string(), message.clone()).await;
        }
    }

    async fn notify_connection(&self, user_id: &str, state: &str) {
        self.dispatch(
            user_id,
            serde_json::json!({ "channel": CONNECTION, "message": state }),
        )
        .await;
    }
}

impl Default for Messenger {
    fn default() -> Self {
        Self::new()
    }
}

/// `GET /ws/{user_id}` -- placeholder auth: a real version reads a bearer
/// token the same way [`crate::core::auth::require_bearer`] does, since a
/// handshake carries no header to check. One socket per browser tab,
/// carrying every channel.
pub async fn upgrade_route(
    Extension(messenger): Extension<Messenger>,
    Path(user_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    use axum::response::IntoResponse;
    ws.on_upgrade(move |socket| async move {
        messenger.notify_connection(&user_id, CONNECTED).await;
        let inbound = messenger.clone();
        let id = user_id.clone();
        messenger
            .hub
            .bind(socket, user_id.clone(), move |frame| {
                let messenger = inbound.clone();
                let id = id.clone();
                async move { messenger.dispatch(&id, frame).await }
            })
            .await;
        messenger.notify_connection(&user_id, DISCONNECTED).await;
    })
    .into_response()
}
