//! Outbound messages to a user or a room. [`websocket`] is the local-dev
//! hub (one process, one open socket per browser tab); [`connections`] is
//! its deployed counterpart, the `connections` table API Gateway's Lambda
//! reads to know who else is listening; [`email`] is a stub that prints to
//! the console. Callers (`chat`, the invite flows) talk to [`Messenger`]
//! only — nothing outside this module touches a raw socket, `connections`,
//! or the WebSocket Management API directly.

pub mod connections;
pub mod email;
pub mod websocket;

use std::future::Future;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Extension, Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::Value;

use crate::auth::AuthState;
use crate::core::apigw_management::{ApiGatewayManagement, ApiGatewayManagementError};
use crate::core::db::{Db, DbError};

/// Every configured channel. Reaches handlers as an `Extension`. Cheap to
/// clone: `Hub` is an `Arc`, `EmailChannel` carries no state.
#[derive(Clone, Default)]
pub struct Messenger {
    websocket: websocket::Hub,
    email: email::EmailChannel,
}

impl Messenger {
    pub fn new() -> Self {
        Self::default()
    }

    /// The live websocket hub, for a feature (chat) that runs its own room
    /// rather than one-at-a-time delivery through [`notify`].
    pub fn websocket(&self) -> &websocket::Hub {
        &self.websocket
    }

    /// The email channel directly, for a caller wording its own body — the
    /// invite flow's full templated email, not `push_toast`'s short one.
    pub fn email(&self) -> &email::EmailChannel {
        &self.email
    }

    /// Deliver `body` to `to` on every channel, best-effort. Unused so far
    /// (every caller wants a differently-worded body per channel).
    #[allow(dead_code)]
    pub async fn notify(&self, to: &str, body: &Value) {
        self.websocket.send(to, body).await;
        self.email.send(to, body).await;
    }

    /// Push a live toast to every open connection `to` has, regardless of
    /// room — the local hub and the deployed `connections` table are each
    /// populated in exactly one of those (never both), so both calls below
    /// are safe to make unconditionally.
    pub async fn push_toast(
        &self,
        db: &Db,
        management: &ApiGatewayManagement,
        to: &str,
        body: &Value,
    ) {
        self.websocket.send(to, body).await;
        if let Ok(connection_ids) = connections::in_user(db, to).await {
            self.deliver(db, management, connection_ids, body).await;
        }
    }

    /// Deployed `$connect`: record a newly joined connection. `org_id`
    /// absent means a presence-only join (see [`connections`]'s module doc).
    pub async fn join(
        &self,
        db: &Db,
        connection_id: &str,
        email: &str,
        org_id: Option<&str>,
    ) -> Result<(), DbError> {
        connections::record(db, connection_id, email, org_id).await
    }

    /// Deployed `$disconnect`, or lazy cleanup once a push comes back `Gone`.
    pub async fn leave(&self, db: &Db, connection_id: &str) -> Result<(), DbError> {
        connections::remove(db, connection_id).await
    }

    /// Who sent a deployed message and which room (if any) they're in — a
    /// message event carries only a connection id, nothing else.
    pub async fn sender(
        &self,
        db: &Db,
        connection_id: &str,
    ) -> Result<Option<connections::Connection>, DbError> {
        connections::by_connection_id(db, connection_id).await
    }

    /// Deployed fan-out: push `body` to every other open connection in
    /// `org_id`'s room. `from` is excluded so a sender doesn't get an echo.
    pub async fn broadcast(
        &self,
        db: &Db,
        management: &ApiGatewayManagement,
        org_id: &str,
        from: &str,
        body: &Value,
    ) {
        let Ok(peers) = connections::in_org(db, org_id).await else {
            return;
        };
        let recipients = peers
            .into_iter()
            .map(|peer| peer.connection_id)
            .filter(|id| id != from);
        self.deliver(db, management, recipients, body).await;
    }

    /// Accept a local-dev WebSocket upgrade and bind it to the hub under
    /// `email`, so callers never touch the raw socket type. `on_message`
    /// runs once per frame the client sends, until it disconnects.
    pub fn join_local<F, Fut>(
        &self,
        upgrade: WebSocketUpgrade,
        email: String,
        on_message: F,
    ) -> Response
    where
        F: Fn(Value) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let hub = self.websocket.clone();
        upgrade
            .on_upgrade(move |socket| async move {
                hub.bind(socket, email, on_message).await;
            })
            .into_response()
    }

    /// Push `body` down every listed connection. A `Gone` reply means the
    /// client vanished without a `$disconnect`, so the row is dropped rather
    /// than left to fail every future push; any other error is logged and
    /// otherwise ignored — delivery here is always best effort.
    async fn deliver(
        &self,
        db: &Db,
        management: &ApiGatewayManagement,
        connection_ids: impl IntoIterator<Item = String>,
        body: &Value,
    ) {
        let Ok(bytes) = serde_json::to_vec(body) else {
            return;
        };
        for connection_id in connection_ids {
            match management.post(&connection_id, &bytes).await {
                Ok(()) => {}
                Err(ApiGatewayManagementError::Gone) => {
                    if let Err(err) = self.leave(db, &connection_id).await {
                        tracing::warn!("messenger: could not remove a stale connection: {err:?}");
                    }
                }
                Err(err) => tracing::warn!("messenger: could not push to a connection: {err}"),
            }
        }
    }
}

/// Query string on a local-dev WebSocket upgrade: a handshake carries no
/// header, so the bearer token rides as `?token=...` instead.
#[derive(Debug, Deserialize)]
pub struct WsAuth {
    pub token: String,
}

/// `GET /chat/ws` — a room-less local-dev socket, no chat involved. Lets
/// [`Messenger::push_toast`] reach someone who isn't a member of any room yet.
pub async fn presence(
    State(auth): State<AuthState>,
    Extension(messenger): Extension<Messenger>,
    Query(params): Query<WsAuth>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let claims = match auth.verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };
    messenger.join_local(upgrade, claims.sub, |_frame| async {})
}
