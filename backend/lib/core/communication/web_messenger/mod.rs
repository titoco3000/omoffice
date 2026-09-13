//! Messaging, as one interface with two directions, over one of two
//! transports chosen at compile time, never at runtime.
//!
//! **Out**: [`Messenger::send`] takes a channel name, a JSON message and a
//! list of recipients (user ids); it reaches every open socket a recipient
//! has. **In**: [`Messenger::on_message`] registers a handler on a channel,
//! called once per frame a client sends on it -- every handler registered
//! on that channel runs, not just the first or the last, so two features
//! can both care about the same channel without one silently displacing
//! the other's handler.
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
//! carries every channel a client cares about.
//!
//! **Two transports, two builds -- never both in the same one.** `make
//! run`/`cargo run` (no `lambda` feature) compiles [`local_web_socket`]: a
//! plain in-process hashtable, for as long as this process runs, with
//! nothing durable to keep in sync -- the process dying is the cleanup.
//! `cargo lambda build --features lambda` (what `make build`/`make deploy`
//! pass) compiles [`web_socket`] instead: a deployed Lambda never holds a
//! socket at all, `infra/messenger.tf`'s WebSocket API does, and each
//! `$connect`/`$default`/`$disconnect` event is its own, independent
//! invocation sharing no memory with the one before it, so "who does this
//! connection belong to" has to live in the database (`ws_connections`)
//! and outbound frames have to go through the Management API rather than a
//! socket this process holds.
//!
//! The two are not just different code paths reached by a runtime check --
//! see `Cargo.toml`'s `lambda` feature: `local_web_socket` is the *only*
//! transport a local build even compiles (`lambda_http` and
//! `aws-sdk-apigatewaymanagement` are optional, gated on `lambda`, and
//! never pulled in without it), and `web_socket` is the only one a Lambda
//! build compiles. Neither binary contains code for a transport it can
//! never use, so there is nothing to allocate, lock, or query for the
//! other one -- not "usually skipped at runtime", genuinely absent. An
//! earlier version of this module ran both transports behind a single
//! runtime check instead; see the git history if curious why that changed.

#[cfg(feature = "lambda")]
pub mod web_socket;

#[cfg(not(feature = "lambda"))]
pub mod local_web_socket;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::Value;

/// A channel name both the browser and backend agree on, e.g. `"chat"` or
/// `"meeting_chat"`. A feature owns one: it registers a handler for messages
/// coming in on it, and calls [`Messenger::send`] to push messages out.
pub type Channel = &'static str;

/// Not a message a client ever sends -- fired locally when a socket opens or
/// closes, carrying [`CONNECTED`]/[`DISCONNECTED`] as its message. Subscribe
/// to it the same way as any other channel to learn who just came online or
/// went away, without touching the socket registry directly.
pub const CONNECTION: Channel = "connection";
pub const CONNECTED: &str = "connected";
pub const DISCONNECTED: &str = "disconnected";

type Handler = Arc<dyn Fn(Messenger, String, Value) -> BoxFuture<'static, ()> + Send + Sync>;

/// The messaging system. Reaches handlers as an axum `Extension`; cheap to
/// clone, same as [`crate::core::data::db::Db`]. [`Messenger::send`] and
/// [`Messenger::is_connected`] -- the only places that differ between the
/// two transports -- are implemented in whichever of [`web_socket`] or
/// [`local_web_socket`] is actually compiled in, alongside that module's
/// own transport-specific field on this struct; everything below is the
/// shared half, common to both.
#[derive(Clone)]
pub struct Messenger {
    #[cfg(feature = "lambda")]
    db: crate::core::data::db::Db,
    #[cfg(feature = "lambda")]
    management: web_socket::ApiGatewayManagement,
    #[cfg(not(feature = "lambda"))]
    hub: local_web_socket::Hub,
    handlers: Arc<RwLock<HashMap<Channel, Vec<Handler>>>>,
}

impl Messenger {
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
    /// where [`CONNECTION`] itself fires. Called by whichever transport is
    /// compiled in, once it has worked out who actually sent the frame.
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

/// Query string on a socket's URL: a handshake carries no header, so the
/// bearer token rides as `?token=...` instead. Used by both transports'
/// connect handling.
#[derive(Debug, Deserialize)]
pub struct WsAuth {
    pub token: String,
}
