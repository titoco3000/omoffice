//! The deployed transport, and (see the module doc on [`super`]) the only
//! websocket code this build ever compiles: API Gateway's WebSocket API
//! holds the socket, this process only ever reaches it through the
//! Management API, and [`connections`] (the `ws_connections` table) is
//! where "who does this connection id belong to" has to live, since a
//! `$connect`/`$default`/`$disconnect` event is its own Lambda invocation
//! sharing no memory with the one before it.

pub mod connections;

use aws_sdk_apigatewaymanagement::primitives::Blob;
use aws_sdk_apigatewaymanagement::Client;
use axum::body::to_bytes;
use axum::extract::{Extension, Query, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
use lambda_http::request::RequestContext;
use serde_json::Value;

use super::{Channel, Messenger, WsAuth, CONNECTED, DISCONNECTED};
use crate::core::auth;
use crate::core::data::db::Db;

/// The management client, scoped to one WebSocket API's endpoint. `None`
/// if `MESSENGER_MANAGEMENT_ENDPOINT` is somehow unset on a Lambda -- every
/// call then reports [`ApiGatewayManagementError::Unavailable`] rather than
/// panicking.
#[derive(Clone)]
pub struct ApiGatewayManagement {
    client: Option<Client>,
}

impl ApiGatewayManagement {
    /// Built from `MESSENGER_MANAGEMENT_ENDPOINT`, set by `infra/messenger.tf`
    /// on the deployed lambda.
    async fn from_env() -> Self {
        let Ok(endpoint) = std::env::var("MESSENGER_MANAGEMENT_ENDPOINT") else {
            tracing::warn!(
                "apigw management: no MESSENGER_MANAGEMENT_ENDPOINT -- every push will fail"
            );
            return Self { client: None };
        };

        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let config = aws_sdk_apigatewaymanagement::config::Builder::from(&sdk_config)
            .endpoint_url(endpoint)
            .build();
        Self {
            client: Some(Client::from_conf(config)),
        }
    }

    fn client(&self) -> Result<&Client, ApiGatewayManagementError> {
        self.client
            .as_ref()
            .ok_or(ApiGatewayManagementError::Unavailable)
    }

    /// Push `data` down one open connection. `Gone` means it's dead -- the
    /// caller's cue to forget it, not retry.
    async fn post(
        &self,
        connection_id: &str,
        data: &[u8],
    ) -> Result<(), ApiGatewayManagementError> {
        self.client()?
            .post_to_connection()
            .connection_id(connection_id)
            .data(Blob::new(data.to_vec()))
            .send()
            .await
            .map(|_| ())
            .map_err(|err| {
                if err
                    .as_service_error()
                    .is_some_and(|e| e.is_gone_exception())
                {
                    ApiGatewayManagementError::Gone
                } else {
                    upstream(err)
                }
            })
    }
}

/// What [`ApiGatewayManagement::post`] can fail with.
#[derive(Debug)]
enum ApiGatewayManagementError {
    /// No client was built -- `MESSENGER_MANAGEMENT_ENDPOINT` was unset.
    Unavailable,
    /// The connection no longer exists.
    Gone,
    /// Anything the Management API itself rejected.
    Upstream(String),
}

impl std::fmt::Display for ApiGatewayManagementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "no management-api client configured"),
            Self::Gone => write!(f, "connection no longer exists"),
            Self::Upstream(why) => write!(f, "{why}"),
        }
    }
}

fn upstream<E, R>(
    err: aws_sdk_apigatewaymanagement::error::SdkError<E, R>,
) -> ApiGatewayManagementError
where
    E: std::error::Error + 'static,
    R: std::fmt::Debug,
{
    ApiGatewayManagementError::Upstream(
        aws_sdk_apigatewaymanagement::error::DisplayErrorContext(&err)
            .to_string()
            .replace('\n', " "),
    )
}

impl Messenger {
    /// This build's only transport is durable, not held in memory, so
    /// there is no local registry to build here -- just the database
    /// handle and a Management API client scoped from the environment.
    pub async fn new(db: Db) -> Self {
        Self {
            management: ApiGatewayManagement::from_env().await,
            db,
            handlers: Default::default(),
        }
    }

    /// Send `message` on `channel` to each of `to` -- every `apigw`
    /// connection the database knows about. Best effort: a recipient with
    /// no open socket simply misses it.
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
                self.deliver(&id, &bytes).await;
            }
        }
    }

    /// True if `user_id` has at least one open `apigw` connection.
    pub async fn is_connected(&self, user_id: &str) -> bool {
        connections::is_connected(&self.db, user_id)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!("messenger: could not check {user_id}'s connections: {err:?}");
                false
            })
    }

    /// One frame down one connection. A `Gone` reply means the client
    /// vanished without a `$disconnect`, so the row is dropped rather than
    /// left to fail every future push; any other error is logged and
    /// otherwise ignored.
    async fn deliver(&self, id: &str, bytes: &[u8]) {
        match self.management.post(id, bytes).await {
            Ok(()) => {}
            Err(ApiGatewayManagementError::Gone) => self.forget(id).await,
            Err(err) => tracing::warn!("messenger: could not push to a connection: {err}"),
        }
    }

    /// Forget a connection -- the one place [`connections::remove`] is
    /// ever called, so whichever of the two paths finds one gone
    /// (`$disconnect`, or a `Gone` reply while pushing to it) notices
    /// exactly once, the same way, and fires [`super::DISCONNECTED`].
    async fn forget(&self, id: &str) {
        match connections::remove(&self.db, id).await {
            Ok(Some(user_id)) => self.notify_connection(&user_id, DISCONNECTED).await,
            Ok(None) => {}
            Err(err) => tracing::warn!("messenger: could not forget a connection: {err:?}"),
        }
    }
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
    let Ok(Some(user_id)) = connections::by_id(&messenger.db, &connection_id).await else {
        return ok;
    };
    messenger.dispatch(&user_id, frame).await;
    ok
}
