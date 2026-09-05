//! API Gateway's Management API — the other half of a WebSocket API's round
//! trip. A WebSocket API route (`$connect`/`$disconnect`/a message route)
//! only ever gets a *request* to the lambda; pushing something back down a
//! *different* open connection than the one that just invoked the lambda
//! goes through this instead — a plain HTTPS call to
//! `{api}.execute-api.{region}.amazonaws.com/{stage}/@connections/{id}`,
//! not a socket write, because the lambda that handles connection A's
//! message and the lambda that would need to write to connection B's socket
//! are not, in general, the same running process — nothing here is a socket
//! at all.
//!
//! This is deliberately as dumb as `core::chime`/`core::db`: one call
//! ([`ApiGatewayManagement::post`]), the identifier API Gateway wants, and
//! nothing about who's on the other end of a connection or why. `chat/mod.rs`
//! is what knows that.

use aws_sdk_apigatewaymanagement::primitives::Blob;
use aws_sdk_apigatewaymanagement::Client;

/// The management client, scoped to one WebSocket API's endpoint. Cheap to
/// clone; the client is `None` in tests and in a local run, where nothing
/// ever reaches this (see [`ApiGatewayManagement::from_env`]).
#[derive(Clone)]
pub struct ApiGatewayManagement {
    client: Option<Client>,
}

impl ApiGatewayManagement {
    /// No AWS behind it: every call reports [`ApiGatewayManagementError::Unavailable`].
    /// Tests only.
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self { client: None }
    }

    /// Built from `CHAT_MANAGEMENT_ENDPOINT` (terraform sets it on the
    /// deployed lambda only, to the WebSocket API's own management
    /// endpoint). Absent locally: `make run dev` is a plain TCP server that
    /// never receives a WebSocket-API proxy event in the first place (see
    /// `chat/mod.rs`), so nothing ever calls through this outside AWS.
    pub async fn from_env() -> Self {
        let Ok(endpoint) = std::env::var("CHAT_MANAGEMENT_ENDPOINT") else {
            tracing::info!(
                "apigw management: no CHAT_MANAGEMENT_ENDPOINT -- local run, unreachable by a WebSocket API"
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

    /// Push `data` down one open connection.
    /// [`ApiGatewayManagementError::Gone`] means the connection is no longer
    /// live (the client closed without API Gateway ever running
    /// `$disconnect` -- a hard crash, a lost network) — the caller's cue to
    /// forget the connection rather than something to retry.
    pub async fn post(
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
pub enum ApiGatewayManagementError {
    /// No client was built -- tests, or a local run (see [`ApiGatewayManagement::from_env`]).
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
