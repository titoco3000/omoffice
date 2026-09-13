//! API Gateway's Management API: pushing to a WebSocket connection is a
//! plain HTTPS call here, not a socket write.

use aws_sdk_apigatewaymanagement::primitives::Blob;
use aws_sdk_apigatewaymanagement::Client;

/// The management client, scoped to one WebSocket API's endpoint. `None`
/// in tests and local runs.
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

    /// Built from `CHAT_MANAGEMENT_ENDPOINT`, set only on the deployed
    /// lambda. Absent locally: `make run dev` never gets a WebSocket-API event.
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

    /// Push `data` down one open connection. `Gone` means it's dead — the
    /// caller's cue to forget it, not retry.
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
