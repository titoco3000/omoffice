//! Chat: a per-org room with no history.
//!
//! There are two, unrelated implementations below, because "hold this
//! browser's socket open and read from it" and "how this API actually runs
//! once deployed" cannot be the same code:
//!
//! * **[`ws`]/[`handle`]** — **local dev only** (`make run dev`). A real,
//!   persistent process really can hold a `WebSocket` open in a loop; this is
//!   `messenger::websocket::Hub` put to use exactly as documented there.
//! * **[`lambda_entry`]** and its helpers — **the deployed stages**. The HTTP
//!   API the rest of this API runs behind (API Gateway v2, `protocol_type =
//!   "HTTP"`) does not proxy WebSocket upgrades at all, and even the
//!   WebSocket API `infra/chat.tf` adds alongside it never hands the lambda a
//!   socket to hold open: API Gateway terminates the actual connection with
//!   the browser itself, and invokes the lambda once per `$connect`, once
//!   per message, once per `$disconnect` — three unrelated, stateless
//!   invocations that may not even land on the same warm instance. So "who's
//!   in this room" has to be a durable, shared registry
//!   ([`connections`], a table — not [`ws`]'s in-memory `Hub`), and "send
//!   them a message" is a plain HTTPS call
//!   ([`core::apigw_management::ApiGatewayManagement::post`][post]), not a
//!   socket write, because the invocation handling the sender's message is
//!   not, in general, the same process that would need to write to a
//!   recipient's socket.
//!
//! [post]: crate::core::apigw_management::ApiGatewayManagement::post
//!
//! Both are reachable because `infra/chat.tf` puts a *second* API Gateway (a
//! WebSocket API) in front of the same lambda, alongside the existing HTTP
//! one. Locally there is no API Gateway at all, so only [`ws`] is ever
//! reached; deployed, a WebSocket-API event never matches [`ws`]'s route
//! (see [`lambda_entry`]'s doc for how the two are told apart at the door)
//! and only [`lambda_entry`] ever runs.

pub mod connections;

use axum::{
    body::to_bytes,
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Extension, Path, Query, Request, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
use lambda_http::request::RequestContext;
use serde::Deserialize;
use serde_json::json;

use crate::auth::AuthState;
use crate::core::apigw_management::{ApiGatewayManagement, ApiGatewayManagementError};
use crate::core::db::Db;
use crate::messenger::Messenger;
use crate::rbac;

// ---------------------------------------------------------------------
// Local dev: a real, persistent process holding a real socket open.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct WsAuth {
    token: String,
}

/// `GET /chat/ws/{org_id}` — join the org's chat room. **Local dev only**;
/// see the module doc for why the deployed stages never reach this.
///
/// Not registered under `/private`: a browser's `WebSocket` API cannot set
/// an `Authorization` header on the handshake, so `require_bearer` (which
/// only ever looks at that header) has no way to protect this route the way
/// it protects everything else. The handshake instead carries the token as
/// `?token=...` and this handler verifies it itself, against the very same
/// [`AuthState`] the middleware uses — same tokens, same rules, just a
/// different place they're checked.
///
/// Membership is checked once, at connect time — there is no re-check per
/// message, so someone removed from the org mid-conversation keeps talking
/// until they reconnect. Same trade-off `meetings::join` makes by not
/// re-checking anything once a caller has a code.
pub async fn ws(
    State(auth): State<AuthState>,
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Path(org_id): Path<String>,
    Query(params): Query<WsAuth>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let claims = match auth.verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = rbac::require_member(&db, &claims.sub, &org_id).await {
        return err.into_response();
    }

    upgrade
        .on_upgrade(move |socket| handle(socket, messenger, db, org_id, claims.sub))
        .into_response()
}

/// Drives one joined socket: registers it on the hub under `from`, then fans
/// out every `{"text": ...}` frame it sends to the rest of the org's roster.
async fn handle(socket: WebSocket, messenger: Messenger, db: Db, org_id: String, from: String) {
    let hub = messenger.websocket().clone();
    // A separate handle for the closure below: `hub` itself stays borrowed by
    // `bind` for the whole connection, so the per-message fan-out needs its
    // own owned clone rather than sharing that borrow.
    let broadcast_hub = hub.clone();

    hub.bind(socket, from.clone(), move |body| {
        let hub = broadcast_hub.clone();
        let db = db.clone();
        let org_id = org_id.clone();
        let from = from.clone();
        async move {
            let Some(text) = body.get("text").and_then(|v| v.as_str()) else {
                return;
            };
            let Ok(members) = rbac::list_members(&db, &org_id).await else {
                return;
            };
            let recipients: Vec<String> = members
                .into_iter()
                .map(|member| member.email)
                .filter(|email| email != &from)
                .collect();
            hub.send_to(&recipients, &json!({ "from": from, "text": text }))
                .await;
        }
    })
    .await;
}

// ---------------------------------------------------------------------
// Deployed stages: `infra/chat.tf`'s WebSocket API. No held-open socket --
// see the module doc.
// ---------------------------------------------------------------------

/// Query string on the `$connect` URL
/// (`wss://.../{stage}?token=...&org_id=...`) — a browser cannot set a
/// header on a WebSocket handshake, so both ride along as query parameters,
/// verified the same way [`ws`]'s `token` is.
#[derive(Debug, Deserialize)]
struct ConnectParams {
    token: String,
    org_id: String,
}

/// Registered as the router's fallback (see `main.rs`), so it only ever runs
/// on a request nothing else matched.
///
/// A genuinely unmatched HTTP-API path still lands here and gets a plain
/// 404, same as before this existed. `infra/chat.tf`'s WebSocket API is what
/// this is actually for: `$connect`/`$disconnect`/a message all synthesize
/// into *some* path under this lambda that the rest of the router never
/// claims (there's no meaningful path or method for any of them — the only
/// thing that says what kind of event this is is
/// `requestContext.eventType`), so they always fall through to here. What
/// tells them apart from a genuine unmatched HTTP path is the one thing
/// `lambda_http` attaches only to a WebSocket-API event: the
/// [`RequestContext::WebSocket`] extension.
pub async fn lambda_entry(
    State(auth): State<AuthState>,
    Extension(db): Extension<Db>,
    Extension(management): Extension<ApiGatewayManagement>,
    request: Request,
) -> Response {
    let Some(RequestContext::WebSocket(ctx)) =
        request.extensions().get::<RequestContext>().cloned()
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match ctx.event_type.as_deref() {
        Some("CONNECT") => handle_connect(&db, &auth, request.uri(), &ctx).await,
        Some("DISCONNECT") => handle_disconnect(&db, &ctx).await,
        // "MESSAGE", or anything else: there is only one route
        // (`$default`), so any frame that isn't connect/disconnect is a
        // chat message attempt.
        _ => handle_message(&db, &management, &ctx, request.into_body()).await,
    }
}

/// `$connect`: verify the token and org membership carried in the query
/// string, then record the connection. A non-2xx response here is how a
/// WebSocket-API `$connect` route rejects the connection outright — API
/// Gateway never finishes the handshake, and no `$disconnect` follows.
async fn handle_connect(
    db: &Db,
    auth: &AuthState,
    uri: &axum::http::Uri,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
) -> Response {
    let Some(connection_id) = ctx.connection_id.clone() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let params = match Query::<ConnectParams>::try_from_uri(uri) {
        Ok(Query(params)) => params,
        Err(err) => return err.into_response(),
    };

    let claims = match auth.verify(&params.token).await {
        Ok(claims) => claims,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = rbac::require_member(db, &claims.sub, &params.org_id).await {
        return err.into_response();
    }

    match connections::record(db, &connection_id, &claims.sub, &params.org_id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    }
}

/// `$disconnect`: forget the connection. Always answered 200 -- there is
/// nothing left to reject once the client is already gone.
async fn handle_disconnect(db: &Db, ctx: &ApiGatewayWebsocketProxyRequestContext) -> Response {
    if let Some(connection_id) = &ctx.connection_id {
        if let Err(err) = connections::remove(db, connection_id).await {
            tracing::warn!("chat: could not remove connection on disconnect: {err:?}");
        }
    }
    StatusCode::OK.into_response()
}

/// A chat message: look the sender up by the connection id API Gateway just
/// invoked this with (the event itself carries neither the sender nor the
/// room), then push it to every other open connection in the same org.
///
/// Answered 200 regardless of what's inside -- a WebSocket-API message route
/// has nobody synchronously listening for this response the way an HTTP
/// caller would, so there is nothing to report a client-visible error to;
/// anything that goes wrong here is logged instead.
async fn handle_message(
    db: &Db,
    management: &ApiGatewayManagement,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
    body: axum::body::Body,
) -> Response {
    let Some(connection_id) = ctx.connection_id.clone() else {
        return StatusCode::OK.into_response();
    };

    let ok = StatusCode::OK.into_response();

    let Ok(bytes) = to_bytes(body, 64 * 1024).await else {
        return ok;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ok;
    };
    let Some(text) = value.get("text").and_then(|v| v.as_str()) else {
        return ok;
    };

    let Ok(Some(sender)) = connections::by_connection_id(db, &connection_id).await else {
        return ok;
    };
    let Ok(peers) = connections::in_org(db, &sender.org_id).await else {
        return ok;
    };

    let payload = json!({ "from": sender.user_email, "text": text });
    let Ok(payload_bytes) = serde_json::to_vec(&payload) else {
        return ok;
    };

    for peer in peers {
        if peer.connection_id == connection_id {
            continue;
        }
        match management.post(&peer.connection_id, &payload_bytes).await {
            Ok(()) => {}
            // The client vanished without API Gateway ever running
            // `$disconnect` (a hard crash, a lost network) -- the row
            // outlived the socket it named, so drop it now instead of
            // failing every future message into this org on it forever.
            Err(ApiGatewayManagementError::Gone) => {
                if let Err(err) = connections::remove(db, &peer.connection_id).await {
                    tracing::warn!("chat: could not remove a stale connection: {err:?}");
                }
            }
            Err(err) => tracing::warn!("chat: could not push to a connection: {err}"),
        }
    }

    ok
}
