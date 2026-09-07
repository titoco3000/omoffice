//! Chat: a per-org room with no history. All delivery — local hub or
//! deployed WebSocket API — goes through [`Messenger`]; this module only
//! decides who a message goes to and checks org membership.

use axum::{
    body::to_bytes,
    extract::{ws::WebSocketUpgrade, Extension, Path, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
use lambda_http::request::RequestContext;
use serde::Deserialize;
use serde_json::json;

use crate::auth::AuthState;
use crate::core::apigw_management::ApiGatewayManagement;
use crate::core::db::Db;
use crate::core::messenger::{Messenger, WsAuth};
use crate::rbac;

// ---- Local dev: a real, persistent process holding a real socket open ----

/// `GET /chat/ws/{org_id}` — join the org's chat room, local dev only. Not
/// under `/private`: the token rides as `?token=...` since a handshake carries no header.
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

    // The hub, for the fan-out below — separate from the join itself, which
    // `join_local` drives.
    let hub = messenger.websocket().clone();
    let from = claims.sub.clone();

    messenger.join_local(upgrade, claims.sub, move |body| {
        let hub = hub.clone();
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
}

// ---- Deployed stages: `infra/chat.tf`'s WebSocket API ----

/// Query string on the `$connect` URL: a handshake can't carry a header, so
/// both ride as query params. `org_id` absent means a presence connection.
#[derive(Debug, Deserialize)]
struct ConnectParams {
    token: String,
    org_id: Option<String>,
}

/// The router's fallback: catches every WebSocket-API event, told apart
/// from a genuine unmatched HTTP path by the [`RequestContext::WebSocket`] extension.
pub async fn lambda_entry(
    State(auth): State<AuthState>,
    Extension(db): Extension<Db>,
    Extension(messenger): Extension<Messenger>,
    Extension(management): Extension<ApiGatewayManagement>,
    request: Request,
) -> Response {
    let Some(RequestContext::WebSocket(ctx)) =
        request.extensions().get::<RequestContext>().cloned()
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match ctx.event_type.as_deref() {
        Some("CONNECT") => handle_connect(&db, &auth, &messenger, request.uri(), &ctx).await,
        Some("DISCONNECT") => handle_disconnect(&db, &messenger, &ctx).await,
        // Only one route ($default), so anything else is a message attempt.
        _ => handle_message(&db, &messenger, &management, &ctx, request.into_body()).await,
    }
}

/// `$connect`: verify the token, check org membership if present, join the
/// room through [`Messenger`]. A non-2xx here rejects the handshake outright.
async fn handle_connect(
    db: &Db,
    auth: &AuthState,
    messenger: &Messenger,
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

    if let Some(org_id) = &params.org_id {
        if let Err(err) = rbac::require_member(db, &claims.sub, org_id).await {
            return err.into_response();
        }
    }

    match messenger
        .join(db, &connection_id, &claims.sub, params.org_id.as_deref())
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    }
}

/// `$disconnect`: leave the room. Always 200 — nothing left to reject once
/// the client is already gone.
async fn handle_disconnect(
    db: &Db,
    messenger: &Messenger,
    ctx: &ApiGatewayWebsocketProxyRequestContext,
) -> Response {
    if let Some(connection_id) = &ctx.connection_id {
        if let Err(err) = messenger.leave(db, connection_id).await {
            tracing::warn!("chat: could not remove connection on disconnect: {err:?}");
        }
    }
    StatusCode::OK.into_response()
}

/// A chat message: look the sender up, then have [`Messenger`] broadcast it
/// to the rest of their org's room. Always answers 200.
async fn handle_message(
    db: &Db,
    messenger: &Messenger,
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

    let Ok(Some(sender)) = messenger.sender(db, &connection_id).await else {
        return ok;
    };
    // A presence connection has no org to fan a message out to.
    let Some(org_id) = &sender.org_id else {
        return ok;
    };

    let payload = json!({ "from": sender.user_email, "text": text });
    messenger
        .broadcast(db, management, org_id, &connection_id, &payload)
        .await;

    ok
}
