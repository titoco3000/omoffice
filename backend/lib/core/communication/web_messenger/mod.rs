//! Messaging, as one interface with two directions, over either of two
//! transports.
//!
//! **Out**: [`Messenger::send`] takes a channel name, a JSON message and a
//! list of recipients (user ids); it resolves each id to whichever socket(s)
//! it currently has open, from [`connections`]. **In**:
//! [`Messenger::on_message`] registers a handler on a channel, called once
//! per frame a client sends on it -- every handler registered on that
//! channel runs, not just the first or the last, so two features can both
//! care about the same channel without one silently displacing the other's
//! handler.
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
//! **Two transports, one roster.** A local run (`make run`) holds the
//! socket itself: [`web_sockets::Hub`] is a plain in-process registry, and
//! [`upgrade_route`] is a normal axum WebSocket upgrade. A deployed Lambda
//! never holds a socket at all -- `infra/messenger.tf`'s WebSocket API
//! does, and each `$connect`/`$default`/`$disconnect` event is its own,
//! independent invocation with no memory shared with the one before it.
//! [`lambda_entry`] is the router's fallback that catches those events
//! (told apart from a genuine unmatched HTTP path by the [`lambda_http`]
//! `RequestContext` extension), and pushes outbound frames through
//! [`apigw_management::ApiGatewayManagement`] instead of the in-process hub
//! -- private to this module, since nothing outside it needs to know that
//! transport exists. [`connections`] (the `ws_connections` table) is what
//! makes the two transports interchangeable to everything above this
//! module: "which sockets does user X have open" is one query, regardless
//! of which invocation opened them, and which of the two transports a given
//! id is actually on is answered at delivery time (try the hub, fall back
//! to the Management API) rather than stored.

mod apigw_management;
pub mod connections;
pub mod web_sockets;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use axum::body::to_bytes;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Extension, Query, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::future::BoxFuture;
use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
use lambda_http::request::RequestContext;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::core::auth;
use crate::core::data::db::Db;
use apigw_management::{ApiGatewayManagement, ApiGatewayManagementError};

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
/// clone, same as [`crate::core::data::db::Db`].
#[derive(Clone)]
pub struct Messenger {
    db: Db,
    management: ApiGatewayManagement,
    hub: web_sockets::Hub,
    handlers: Arc<RwLock<HashMap<Channel, Vec<Handler>>>>,
}

impl Messenger {
    /// `ApiGatewayManagement` is built here, from the environment, rather
    /// than handed in -- a caller (`main.rs`) has no reason to know that
    /// transport exists at all, let alone construct it.
    pub async fn new(db: Db) -> Self {
        Self {
            db,
            management: ApiGatewayManagement::from_env().await,
            hub: web_sockets::Hub::default(),
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Drop every local socket row left over from an earlier run. Local-run
    /// startup only; a Lambda never calls this. See [`connections::clear_local`].
    pub async fn clear_local_connections(&self) {
        if let Err(err) = connections::clear_local(&self.db).await {
            tracing::warn!("messenger: could not clear stale local connections: {err:?}");
        }
    }

    /// Send `message` on `channel` to each of `to` (user ids), over
    /// whichever transport each of their sockets is actually on. Best
    /// effort: a recipient with no open socket simply misses it.
    pub async fn send(&self, channel: Channel, to: &[String], message: Value) {
        let frame = serde_json::json!({ "channel": channel, "message": message });
        let Ok(bytes) = serde_json::to_vec(&frame) else {
            return;
        };
        for user_id in to {
            let ids = match connections::for_user(&self.db, user_id).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::warn!("messenger: cannot list {user_id}'s sockets: {err:?}");
                    continue;
                }
            };
            for id in ids {
                self.deliver(&id, &frame, &bytes).await;
            }
        }
    }

    /// True if `user_id` has at least one open socket right now, on either
    /// transport.
    pub async fn is_connected(&self, user_id: &str) -> bool {
        connections::is_connected(&self.db, user_id)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!("messenger: could not check {user_id}'s connections: {err:?}");
                false
            })
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

    /// Look the sender up by connection id, then dispatch -- what both
    /// [`upgrade_route`] and [`lambda_entry`]'s `message` handler reduce to,
    /// once each has its own way of finding a connection id.
    async fn dispatch_from_connection(&self, id: &str, frame: Value) {
        let Ok(Some(user_id)) = connections::by_id(&self.db, id).await else {
            return;
        };
        self.dispatch(&user_id, frame).await;
    }

    async fn notify_connection(&self, user_id: &str, state: &str) {
        self.dispatch(
            user_id,
            serde_json::json!({ "channel": CONNECTION, "message": state }),
        )
        .await;
    }

    /// One frame down one socket. Tries the in-process hub first; not
    /// registered there means it must be an `apigw` connection instead, so
    /// this falls back to the Management API -- see the module doc on why
    /// nothing stores which of the two a connection is actually on. A
    /// `Gone` reply means the client vanished without a `$disconnect`, so
    /// the row is dropped rather than left to fail every future push; any
    /// other error is logged and otherwise ignored.
    async fn deliver(&self, id: &str, frame: &Value, bytes: &[u8]) {
        if self.hub.send(id, frame).await {
            return;
        }
        match self.management.post(id, bytes).await {
            Ok(()) => {}
            Err(ApiGatewayManagementError::Gone) => self.forget(id).await,
            Err(err) => tracing::warn!("messenger: could not push to a connection: {err}"),
        }
    }

    /// Forget a socket -- the one place [`connections::remove`] is ever
    /// called, so whichever of the three paths finds a connection gone
    /// (`$disconnect`, a closed local socket, a `Gone` reply while pushing
    /// to it) notices exactly once, the same way, and fires
    /// [`DISCONNECTED`].
    async fn forget(&self, id: &str) {
        match connections::remove(&self.db, id).await {
            Ok(Some(user_id)) => self.notify_connection(&user_id, DISCONNECTED).await,
            Ok(None) => {}
            Err(err) => tracing::warn!("messenger: could not forget a connection: {err:?}"),
        }
    }
}

/// Query string on a socket's URL: a handshake carries no header, so the
/// bearer token rides as `?token=...` instead.
#[derive(Debug, Deserialize)]
pub struct WsAuth {
    pub token: String,
}

/// `GET /ws?token=...` -- the local-dev socket, one per browser tab,
/// carrying every channel; this process holds it open itself.
pub async fn upgrade_route(
    Extension(messenger): Extension<Messenger>,
    Query(params): Query<WsAuth>,
    ws: WebSocketUpgrade,
) -> Response {
    let claims = match auth::verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };

    ws.on_upgrade(move |socket| async move {
        // Prefixed so `connections::clear_local` can find it again at the
        // next local startup -- see that function's doc.
        let id = format!("local-{}", Uuid::new_v4());
        if let Err(err) = connections::record(&messenger.db, &id, &claims.sub).await {
            tracing::warn!("messenger: cannot record a local connection: {err:?}");
            return;
        }
        messenger.notify_connection(&claims.sub, CONNECTED).await;

        let inbound = messenger.clone();
        let bound_id = id.clone();
        messenger
            .hub
            .bind(socket, id.clone(), move |frame| {
                let messenger = inbound.clone();
                let id = bound_id.clone();
                async move { messenger.dispatch_from_connection(&id, frame).await }
            })
            .await;

        messenger.forget(&id).await;
    })
    .into_response()
}

/// The router's fallback: catches every WebSocket-API event, told apart
/// from a genuine unmatched HTTP path by the [`RequestContext::WebSocket`]
/// extension. `infra/messenger.tf` points that API at this same lambda.
pub async fn lambda_entry(
    Extension(messenger): Extension<Messenger>,
    request: Request,
) -> Response {
    let Some(RequestContext::WebSocket(ctx)) =
        request.extensions().get::<RequestContext>().cloned()
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match ctx.event_type.as_deref() {
        Some("CONNECT") => connect(&messenger, request.uri(), &ctx).await,
        Some("DISCONNECT") => disconnect(&messenger, &ctx).await,
        // Only one route ($default), so anything else is a message.
        _ => message(&messenger, &ctx, request.into_body()).await,
    }
}

/// `$connect`: verify the token and record the connection. A non-2xx here
/// rejects the handshake outright.
async fn connect(
    messenger: &Messenger,
    uri: &axum::http::Uri,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
) -> Response {
    let Some(connection_id) = ctx.connection_id.clone() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let params = match Query::<WsAuth>::try_from_uri(uri) {
        Ok(Query(params)) => params,
        Err(err) => return err.into_response(),
    };
    let claims = match auth::verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };

    match connections::record(&messenger.db, &connection_id, &claims.sub).await {
        Ok(()) => {
            messenger.notify_connection(&claims.sub, CONNECTED).await;
            StatusCode::OK.into_response()
        }
        Err(err) => err.into_response(),
    }
}

/// `$disconnect`: forget the connection. Always 200 -- nothing left to
/// reject once the client is already gone.
async fn disconnect(
    messenger: &Messenger,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
) -> Response {
    if let Some(connection_id) = &ctx.connection_id {
        messenger.forget(connection_id).await;
    }
    StatusCode::OK.into_response()
}

/// A frame on the deployed socket. Always answers 200: the sender is told
/// nothing about delivery either way.
async fn message(
    messenger: &Messenger,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
    body: axum::body::Body,
) -> Response {
    let ok = StatusCode::OK.into_response();
    let Some(connection_id) = ctx.connection_id.clone() else {
        return ok;
    };
    let Ok(bytes) = to_bytes(body, 64 * 1024).await else {
        return ok;
    };
    let Ok(frame) = serde_json::from_slice::<Value>(&bytes) else {
        return ok;
    };
    messenger
        .dispatch_from_connection(&connection_id, frame)
        .await;
    ok
}
