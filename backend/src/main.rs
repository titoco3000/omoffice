use axum::{
    extract::{Extension, Path},
    middleware,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};

mod auth;

use auth::{AuthConfig, Claims, KeySource};

/// Health/root endpoint. Same handler serves local HTTP and Lambda.
async fn root() -> Json<Value> {
    Json(json!({ "message": "hello world!" }))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Behind `/private`, so the middleware guarantees the claims are present.
async fn private_me(Extension(claims): Extension<Claims>) -> Json<Value> {
    Json(json!({ "email": claims.sub, "expires_at": claims.exp }))
}

/// Body of `POST /private/sample-post/{var}`.
#[derive(Debug, Deserialize)]
struct SamplePost {
    name: String,
    male: bool,
}

/// Sample write route: one path parameter plus a JSON body, echoed back with
/// the caller taken from the verified token.
async fn private_sample_post(
    Extension(claims): Extension<Claims>,
    Path(var): Path<String>,
    Json(body): Json<SamplePost>,
) -> Json<Value> {
    Json(json!({
        "received": { "var": var, "name": body.name, "male": body.male },
        "by": claims.sub,
    }))
}

/// Wide-open CORS. The API is a public, token-authenticated JSON endpoint with
/// no cookies or other ambient credentials, so any origin may call it; the
/// bearer token is what grants access. Narrow this to the frontend origins if
/// the API ever starts trusting the browser's identity.
fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

fn app(keys: KeySource) -> Router {
    let auth = AuthConfig::new(keys)
        // Every route under these prefixes needs a valid bearer token.
        .protect(vec!["/private"])
        // Only these emails may ask for a token.
        .allow_emails(vec!["admin@omoffice.example", "dev@omoffice.example"])
        .build();

    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/auth/request-key/{email}", get(auth::request_key))
        .route("/private/me", get(private_me))
        .route("/private/sample-post/{var}", post(private_sample_post))
        // One layer for the whole router; it only challenges protected prefixes.
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            auth::require_bearer,
        ))
        // Outermost, so a CORS preflight is answered before the auth middleware
        // can reject it: an OPTIONS request carries no Authorization header.
        .layer(cors())
        .with_state(auth)
}

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    // AWS_LAMBDA_RUNTIME_API is only set by the Lambda execution environment.
    let in_lambda = std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok();
    let keys = KeySource::from_env(in_lambda).await;

    if in_lambda {
        return lambda_http::run(app(keys)).await;
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("listening on http://localhost:{port}");
    axum::serve(listener, app(keys)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const KEY: &str = "test-key";

    async fn call(request: Request<Body>) -> (StatusCode, Value) {
        let response = app(KeySource::fixed(KEY))
            .oneshot(request)
            .await
            .expect("router responds");
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn post_json(uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    fn get_with_auth(uri: &str, header_value: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("authorization", header_value)
            .body(Body::empty())
            .unwrap()
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    async fn token_for(email: &str) -> String {
        let (status, body) = call(get(&format!("/auth/request-key/{email}"))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn public_routes_need_no_token() {
        let (status, body) = call(get("/")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["message"], "hello world!");

        let (status, _) = call(get("/health")).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn whitelisted_email_gets_a_48h_token() {
        let (status, body) = call(get("/auth/request-key/dev@omoffice.example")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email"], "dev@omoffice.example");
        assert_eq!(body["expires_in"], 48 * 60 * 60);
        assert!(body["expires_at"].as_u64().unwrap() > now());
    }

    #[tokio::test]
    async fn unlisted_email_is_rejected() {
        let (status, body) = call(get("/auth/request-key/stranger@example.com")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "email_not_allowed");
    }

    #[tokio::test]
    async fn protected_route_accepts_a_valid_token() {
        let token = token_for("dev@omoffice.example").await;
        let (status, body) = call(get_with_auth("/private/me", &format!("Bearer {token}"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email"], "dev@omoffice.example");
    }

    #[tokio::test]
    async fn protected_route_rejects_a_missing_token() {
        let (status, body) = call(get("/private/me")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "missing_token");
    }

    #[tokio::test]
    async fn protected_route_rejects_a_non_bearer_scheme() {
        let (status, body) = call(get_with_auth("/private/me", "Basic abc123")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "missing_token");
    }

    #[tokio::test]
    async fn protected_route_rejects_a_malformed_token() {
        let (status, body) = call(get_with_auth("/private/me", "Bearer not-a-jwt")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "invalid_token");
    }

    #[tokio::test]
    async fn protected_route_rejects_a_token_signed_with_another_key() {
        let token = auth::sign_for_test("some-other-key", "dev@omoffice.example", now() + 3600);
        let (status, body) = call(get_with_auth("/private/me", &format!("Bearer {token}"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "invalid_token");
    }

    #[tokio::test]
    async fn protected_route_rejects_an_expired_token() {
        // Past the default 60s leeway.
        let token = auth::sign_for_test(KEY, "dev@omoffice.example", now() - 3600);
        let (status, body) = call(get_with_auth("/private/me", &format!("Bearer {token}"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "expired_token");
    }

    #[tokio::test]
    async fn sample_post_echoes_the_path_param_and_the_body() {
        let token = token_for("dev@omoffice.example").await;
        let (status, body) = call(post_json(
            "/private/sample-post/abc",
            Some(&token),
            json!({ "name": "ada", "male": false }),
        ))
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["received"]["var"], "abc");
        assert_eq!(body["received"]["name"], "ada");
        assert_eq!(body["received"]["male"], false);
        assert_eq!(body["by"], "dev@omoffice.example");
    }

    #[tokio::test]
    async fn sample_post_is_protected() {
        let (status, body) = call(post_json(
            "/private/sample-post/abc",
            None,
            json!({ "name": "ada", "male": false }),
        ))
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "missing_token");
    }

    /// The browser sends this before any authenticated call; it carries no
    /// Authorization header, so CORS has to answer it ahead of the auth layer.
    #[tokio::test]
    async fn a_preflight_for_a_protected_route_is_allowed() {
        let request = Request::builder()
            .method("OPTIONS")
            .uri("/private/sample-post/abc")
            .header("origin", "http://localhost:5173")
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "authorization,content-type",
            )
            .body(Body::empty())
            .unwrap();
        let response = app(KeySource::fixed(KEY)).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
    }

    #[tokio::test]
    async fn unauthorized_responses_carry_a_bearer_challenge() {
        let response = app(KeySource::fixed(KEY))
            .oneshot(get("/private/me"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()["www-authenticate"],
            r#"Bearer error="missing_token""#
        );
    }
}
