//! `map-authorizer`: the office map's AppSync Events Lambda authorizer.
//! Deliberately thin -- no database, no shared state, just
//! [`realtime::authorize`] on each event AppSync hands it; see
//! `omoffice_api::features::map::realtime` for what it allows.

use lambda_runtime::{service_fn, Error, LambdaEvent};
use omoffice_api::core::auth;
use omoffice_api::features::map::realtime::{self, AuthorizerRequest, AuthorizerResponse};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();
    lambda_runtime::run(service_fn(handle)).await
}

async fn handle(event: LambdaEvent<AuthorizerRequest>) -> Result<AuthorizerResponse, Error> {
    Ok(realtime::authorize(&event.payload, auth::now()))
}
