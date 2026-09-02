use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

/// Health/root endpoint. Same handler serves local HTTP and Lambda.
async fn root() -> Json<Value> {
    Json(json!({ "message": "hello world!" }))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

fn app() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
}

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    // AWS_LAMBDA_RUNTIME_API is only set by the Lambda execution environment.
    if std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok() {
        return lambda_http::run(app()).await;
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("listening on http://localhost:{port}");
    axum::serve(listener, app()).await?;
    Ok(())
}
