//! Messaging, as one interface with two directions.
//!
//! **Out**: [`Messenger::send`] and its `_to_users_on_org` /
//! `_to_users_on_meeting` variants take a JSON message, a channel name, the
//! transports to use ([`Via`]) and an audience. **In**:
//! [`Messenger::on_message`] registers a handler per channel, called once
//! per frame a client sends on it.
//!
//! Nothing above this module knows a socket exists. A caller names a
//! channel and an audience; this module resolves the audience through
//! [`active_users`], and delivers over whichever transport each row is on —
//! the in-process [`websocket::Hub`] for a local run, API Gateway's
//! Management API for a deployed one. [`email`] is the other [`Via`], a
//! console stub for now.
//!
//! The wire frame is `{"channel": ..., "message": ...}` in both directions,
//! so one socket carries every channel a client cares about.

pub mod active_users;
pub mod email;
pub mod websocket;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use axum::body::to_bytes;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Extension, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::future::BoxFuture;
use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
use lambda_http::request::RequestContext;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::AuthState;
use crate::core::apigw_management::{ApiGatewayManagement, ApiGatewayManagementError};
use crate::core::db::{Db, DbError};
use active_users::{ActiveUser, APIGW, LOCAL};

/// How a message travels. A caller lists the ones it wants; unlisted
/// transports are simply not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Websocket,
    Email,
}

/// Who sent an incoming frame, and the context they were in when they did —
/// what a channel handler is told besides the message itself.
#[derive(Debug, Clone)]
pub struct Origin {
    pub user: String,
    pub connection_id: String,
    pub org: Option<String>,
    pub meeting: Option<String>,
}

impl From<ActiveUser> for Origin {
    fn from(row: ActiveUser) -> Self {
        Self {
            user: row.user_email,
            connection_id: row.connection_id,
            org: row.org_id,
            meeting: row.meeting,
        }
    }
}

/// A registered channel handler. It is handed the [`Messenger`] rather than
/// capturing one, so a handler that answers by sending does not close a
/// reference cycle back into the `Arc` it is stored in.
type Handler = Arc<dyn Fn(Messenger, Value, Origin) -> BoxFuture<'static, ()> + Send + Sync>;

/// The messaging system. Reaches handlers as an `Extension`; cheap to clone
/// (every field is an `Arc` or carries no state).
#[derive(Clone)]
pub struct Messenger {
    db: Db,
    management: ApiGatewayManagement,
    hub: websocket::Hub,
    email: email::EmailChannel,
    handlers: Arc<RwLock<HashMap<String, Handler>>>,
}

impl Messenger {
    /// Both dependencies live here, not at the call sites: a caller sends
    /// to a channel and never sees a database or the Management API.
    pub fn new(db: Db, management: ApiGatewayManagement) -> Self {
        Self {
            db,
            management,
            hub: websocket::Hub::default(),
            email: email::EmailChannel,
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Drop every local socket row left over from an earlier run. Local
    /// only: see [`active_users::clear_transport`].
    pub async fn clear_local_connections(&self) {
        if let Err(err) = active_users::clear_transport(&self.db, LOCAL).await {
            tracing::warn!("messenger: could not clear stale local connections: {err:?}");
        }
    }

    /// The database, for a channel handler that needs one to decide whether
    /// a frame is allowed (`presence`'s membership check, say).
    pub fn db(&self) -> &Db {
        &self.db
    }

    // ---- Outbound ----

    /// Send `message` on `channel` to each of `to` (email addresses), over
    /// every listed transport. Best effort throughout: a recipient with no
    /// socket open simply misses the websocket copy.
    pub async fn send(&self, message: &Value, channel: &str, via: &[Via], to: &[String]) {
        if via.contains(&Via::Email) {
            for address in to {
                self.email.send(address, channel, message).await;
            }
        }
        if via.contains(&Via::Websocket) {
            let mut rows = Vec::new();
            for address in to {
                match active_users::for_user(&self.db, address).await {
                    Ok(mut found) => rows.append(&mut found),
                    Err(err) => {
                        tracing::warn!("messenger: cannot list {address}'s sockets: {err:?}")
                    }
                }
            }
            self.deliver(rows, channel, message).await;
        }
    }

    /// Send to everyone whose reported context is meeting `meeting` — the
    /// people actually in it right now, not everyone who ever joined.
    #[allow(dead_code)] // No feature broadcasts per meeting yet; `chat` is per org.
    pub async fn send_to_users_on_meeting(
        &self,
        message: &Value,
        channel: &str,
        via: &[Via],
        meeting: &str,
    ) {
        let rows = match active_users::in_meeting(&self.db, meeting).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!("messenger: cannot list meeting {meeting}: {err:?}");
                return;
            }
        };
        self.fan_out(rows, message, channel, via).await;
    }

    /// Send to everyone whose reported context is org `org` — the org's
    /// present members, the audience a chat room means.
    pub async fn send_to_users_on_org(
        &self,
        message: &Value,
        channel: &str,
        via: &[Via],
        org: &str,
    ) {
        let rows = match active_users::in_org(&self.db, org).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!("messenger: cannot list org {org}: {err:?}");
                return;
            }
        };
        self.fan_out(rows, message, channel, via).await;
    }

    /// Register `handler` for `channel`. Registering twice replaces the
    /// first — a channel has one owner. Called once per feature at startup.
    pub fn on_message<F, Fut>(&self, channel: &str, handler: F)
    where
        F: Fn(Messenger, Value, Origin) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler: Handler = Arc::new(move |messenger, message, origin| {
            Box::pin(handler(messenger, message, origin))
        });
        self.handlers
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(channel.to_string(), handler);
    }

    /// Replace what one connection reports about itself. Both halves are
    /// always written: a report is the whole context, not a patch, so
    /// leaving a meeting is `meeting: None` rather than a separate call.
    /// Whoever calls this has already decided the claim is allowed.
    pub async fn set_context(
        &self,
        connection_id: &str,
        org: Option<&str>,
        meeting: Option<&str>,
    ) -> Result<(), DbError> {
        active_users::set_context(&self.db, connection_id, org, meeting).await
    }

    // ---- Transports: everything below is what "no caller sees a socket" costs ----

    /// Resolve `rows` to unique addresses for [`Via::Email`], then deliver
    /// the websocket copy — the shared tail of the two audience helpers.
    async fn fan_out(&self, rows: Vec<ActiveUser>, message: &Value, channel: &str, via: &[Via]) {
        if via.contains(&Via::Email) {
            let mut addressed: Vec<&str> = Vec::new();
            for row in &rows {
                if !addressed.contains(&row.user_email.as_str()) {
                    addressed.push(&row.user_email);
                    self.email.send(&row.user_email, channel, message).await;
                }
            }
        }
        if via.contains(&Via::Websocket) {
            self.deliver(rows, channel, message).await;
        }
    }

    /// Push one message down every listed socket, over whichever transport
    /// each row is on. A `Gone` reply means the client vanished without a
    /// `$disconnect`, so the row is dropped rather than left to fail every
    /// future push; any other error is logged and otherwise ignored.
    async fn deliver(&self, rows: Vec<ActiveUser>, channel: &str, message: &Value) {
        let frame = envelope(channel, message);
        let Ok(bytes) = serde_json::to_vec(&frame) else {
            return;
        };
        for row in rows {
            if row.transport == LOCAL {
                self.hub.send(&row.connection_id, &frame).await;
                continue;
            }
            match self.management.post(&row.connection_id, &bytes).await {
                Ok(()) => {}
                Err(ApiGatewayManagementError::Gone) => {
                    if let Err(err) = active_users::remove(&self.db, &row.connection_id).await {
                        tracing::warn!("messenger: could not remove a stale connection: {err:?}");
                    }
                }
                Err(err) => tracing::warn!("messenger: could not push to a connection: {err}"),
            }
        }
    }

    /// One incoming frame, from whichever transport carried it: look the
    /// sender up, then hand the message to that channel's handler.
    async fn dispatch(&self, connection_id: &str, frame: Value) {
        let Some(channel) = frame.get("channel").and_then(Value::as_str) else {
            return;
        };
        let handler = self
            .handlers
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(channel)
            .cloned();
        let Some(handler) = handler else {
            tracing::debug!("messenger: nothing registered for channel {channel}");
            return;
        };
        let Ok(Some(row)) = active_users::by_connection_id(&self.db, connection_id).await else {
            return;
        };
        let message = frame.get("message").cloned().unwrap_or(Value::Null);
        handler(self.clone(), message, Origin::from(row)).await;
    }
}

/// The wire frame, both directions.
fn envelope(channel: &str, message: &Value) -> Value {
    json!({ "channel": channel, "message": message })
}

/// Query string on a socket's URL: a handshake carries no header, so the
/// bearer token rides as `?token=...` instead.
#[derive(Debug, Deserialize)]
pub struct WsAuth {
    pub token: String,
}

/// `GET /ws?token=...` — the local-dev socket. One per browser tab,
/// carrying every channel; the process holds it open itself.
pub async fn ws(
    State(auth): State<AuthState>,
    Extension(messenger): Extension<Messenger>,
    Query(params): Query<WsAuth>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let claims = match auth.verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };

    upgrade
        .on_upgrade(move |socket| async move {
            // Local sockets go in the same table as deployed ones, so one
            // roster query answers "who is listening" in either mode.
            let connection_id = format!("local-{}", Uuid::new_v4());
            if let Err(err) =
                active_users::record(&messenger.db, &connection_id, &claims.sub, LOCAL).await
            {
                tracing::warn!("messenger: cannot record a local connection: {err:?}");
                return;
            }

            let inbound = messenger.clone();
            let id = connection_id.clone();
            messenger
                .hub
                .bind(socket, connection_id.clone(), move |frame| {
                    let messenger = inbound.clone();
                    let id = id.clone();
                    async move { messenger.dispatch(&id, frame).await }
                })
                .await;

            if let Err(err) = active_users::remove(&messenger.db, &connection_id).await {
                tracing::warn!("messenger: cannot forget a closed local connection: {err:?}");
            }
        })
        .into_response()
}

/// The router's fallback: catches every WebSocket-API event, told apart
/// from a genuine unmatched HTTP path by the [`RequestContext::WebSocket`]
/// extension. `infra/chat.tf` points that API at this same lambda.
pub async fn lambda_entry(
    State(auth): State<AuthState>,
    Extension(messenger): Extension<Messenger>,
    request: Request,
) -> Response {
    let Some(RequestContext::WebSocket(ctx)) =
        request.extensions().get::<RequestContext>().cloned()
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match ctx.event_type.as_deref() {
        Some("CONNECT") => connect(&auth, &messenger, request.uri(), &ctx).await,
        Some("DISCONNECT") => disconnect(&messenger, &ctx).await,
        // Only one route ($default), so anything else is a message.
        _ => message(&messenger, &ctx, request.into_body()).await,
    }
}

/// `$connect`: verify the token and record the connection. A non-2xx here
/// rejects the handshake outright. Context comes later, over the socket.
async fn connect(
    auth: &AuthState,
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
    let claims = match auth.verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };

    match active_users::record(&messenger.db, &connection_id, &claims.sub, APIGW).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    }
}

/// `$disconnect`: forget the connection. Always 200 — nothing left to
/// reject once the client is already gone.
async fn disconnect(
    messenger: &Messenger,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
) -> Response {
    if let Some(connection_id) = &ctx.connection_id {
        if let Err(err) = active_users::remove(&messenger.db, connection_id).await {
            tracing::warn!("messenger: could not remove connection on disconnect: {err:?}");
        }
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
    messenger.dispatch(&connection_id, frame).await;
    ok
}
