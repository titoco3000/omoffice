use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const KEY: &str = "test-key";

/// A router with a fixed signing key and neither a Chime client nor a
/// database, so nothing here touches AWS.
fn test_app() -> Router {
    app(
        KeySource::fixed(KEY),
        ChimeState::disabled(),
        Db::disabled(),
        Messenger::new(),
        ApiGatewayManagement::disabled(),
    )
}

async fn call(request: Request<Body>) -> (StatusCode, Value) {
    let response = test_app().oneshot(request).await.expect("router responds");
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
async fn request_key_issues_a_token_to_any_email() {
    let (status, body) = call(get("/auth/request-key/dev@omoffice.example")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "dev@omoffice.example");
    // No email whitelist any more, and `TOKEN_TTL` (auth/mod.rs) is 7 days.
    assert_eq!(body["expires_in"], 7 * 24 * 60 * 60);
    assert!(body["expires_at"].as_u64().unwrap() > now());
}

#[tokio::test]
async fn a_never_before_seen_email_also_gets_a_token() {
    let (status, body) = call(get("/auth/request-key/stranger@example.com")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["email"], "stranger@example.com");
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
async fn sample_post_reports_a_missing_database() {
    let token = token_for("dev@omoffice.example").await;
    let (status, body) = call(post_json(
        "/private/sample-post/abc",
        Some(&token),
        json!({ "org_id": "some-org", "name": "ada", "male": false }),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

#[tokio::test]
async fn sample_post_is_protected() {
    let (status, body) = call(post_json(
        "/private/sample-post/abc",
        None,
        json!({ "org_id": "some-org", "name": "ada", "male": false }),
    ))
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn creating_a_meeting_is_protected() {
    let (status, body) = call(post_json("/private/meetings", None, json!({}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn joining_a_meeting_is_protected() {
    let (status, body) = call(post_json(
        "/private/meetings/some-code/attendees",
        None,
        json!({}),
    ))
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn listing_meetings_is_protected() {
    let (status, body) = call(get("/private/meetings")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn listing_meetings_reports_a_missing_database() {
    let token = token_for("dev@omoffice.example").await;
    let (status, body) = call(get_with_auth(
        "/private/meetings?org_id=some-org",
        &format!("Bearer {token}"),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "database_unavailable");
}

/// `create`/`list` both check org membership (`rbac::require_member`) before
/// touching Chime at all, so this never reaches the (also absent) Chime client.
#[tokio::test]
async fn creating_a_meeting_reports_a_missing_database() {
    let token = token_for("dev@omoffice.example").await;
    let (status, body) = call(post_json(
        "/private/meetings",
        Some(&token),
        json!({ "org_id": "some-org" }),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

#[tokio::test]
async fn creating_an_org_is_protected() {
    let (status, body) = call(post_json("/private/orgs", None, json!({ "name": "acme" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn listing_orgs_is_protected() {
    let (status, body) = call(get("/private/orgs")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn leaving_an_org_is_protected() {
    let request = Request::builder()
        .method("DELETE")
        .uri("/private/orgs/some-org/members/me")
        .body(Body::empty())
        .unwrap();
    let (status, body) = call(request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn deleting_an_org_is_protected() {
    let request = Request::builder()
        .method("DELETE")
        .uri("/private/orgs/some-org")
        .body(Body::empty())
        .unwrap();
    let (status, body) = call(request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn billing_route_is_protected() {
    let (status, body) = call(get("/private/orgs/some-org/billing")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn org_routes_report_a_missing_database() {
    let token = token_for("dev@omoffice.example").await;
    let (status, body) = call(post_json(
        "/private/orgs",
        Some(&token),
        json!({ "name": "acme" }),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");

    let (status, body) = call(get_with_auth("/private/orgs", &format!("Bearer {token}"))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

#[tokio::test]
async fn creating_an_org_invite_is_protected() {
    let (status, body) = call(post_json(
        "/private/orgs/some-org/invites",
        None,
        json!({ "email": "invited@example.com" }),
    ))
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn listing_org_invites_is_protected() {
    let (status, body) = call(get("/private/orgs/some-org/invites")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn revoking_an_org_invite_is_protected() {
    let request = Request::builder()
        .method("DELETE")
        .uri("/private/orgs/some-org/invites/some-invite")
        .body(Body::empty())
        .unwrap();
    let (status, body) = call(request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn declining_an_org_invite_is_protected() {
    let (status, body) = call(post_json(
        "/private/invites/org/some-invite/decline",
        None,
        json!({}),
    ))
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

/// Not under `/private`: the magic link needs no bearer token at all (see
/// `rbac::accept_invite_route`).
#[tokio::test]
async fn accepting_an_org_invite_needs_no_token_and_reports_a_missing_database() {
    let (status, body) = call(post_json(
        "/invites/org/some-invite/accept",
        None,
        json!({}),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

#[tokio::test]
async fn creating_a_meeting_invite_is_protected() {
    let (status, body) = call(post_json(
        "/private/meetings/some-code/invites",
        None,
        json!({ "email": "invited@example.com" }),
    ))
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn revoking_a_meeting_invite_is_protected() {
    let request = Request::builder()
        .method("DELETE")
        .uri("/private/meetings/some-code/invites/some-invite")
        .body(Body::empty())
        .unwrap();
    let (status, body) = call(request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn declining_a_meeting_invite_is_protected() {
    let (status, body) = call(post_json(
        "/private/invites/meetings/some-invite/decline",
        None,
        json!({}),
    ))
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

#[tokio::test]
async fn accepting_a_meeting_invite_needs_no_token_and_reports_a_missing_database() {
    let (status, body) = call(post_json(
        "/invites/meetings/some-invite/accept",
        None,
        json!({}),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

#[tokio::test]
async fn pending_invites_is_protected() {
    let (status, body) = call(get("/private/invites")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "missing_token");
}

/// `private_pending_invites` tries `rbac`'s query first, so that's what
/// answers here.
#[tokio::test]
async fn pending_invites_reports_a_missing_database() {
    let token = token_for("dev@omoffice.example").await;
    let (status, body) = call(get_with_auth(
        "/private/invites",
        &format!("Bearer {token}"),
    ))
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

/// The browser sends this before any authenticated call, with no
/// Authorization header, so CORS must answer it ahead of the auth layer.
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
    let response = test_app().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
}

#[tokio::test]
async fn unauthorized_responses_carry_a_bearer_challenge() {
    let response = test_app().oneshot(get("/private/me")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers()["www-authenticate"],
        r#"Bearer error="missing_token""#
    );
}

// Builds the `RequestContext::WebSocket` extension directly rather than
// exercising `lambda_http`'s own event deserialization.
use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
use lambda_http::request::RequestContext;

/// `ApiGatewayWebsocketProxyRequestContext` is `#[non_exhaustive]`, so this
/// builds one via JSON rather than a struct literal.
fn ws_context(event_type: &str, connection_id: &str) -> ApiGatewayWebsocketProxyRequestContext {
    let json = format!(
        r#"{{"eventType":"{event_type}","connectionId":"{connection_id}","connectedAt":0,"requestTimeEpoch":0}}"#
    );
    serde_json::from_str(&json).expect("valid websocket request context fixture")
}

fn ws_event(uri: &str, ctx: ApiGatewayWebsocketProxyRequestContext) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .extension(RequestContext::WebSocket(ctx))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_connect_event_with_a_bad_token_is_rejected() {
    let request = ws_event(
        "/dev?token=not-a-jwt&org_id=some-org",
        ws_context("CONNECT", "conn-1"),
    );
    let (status, body) = call(request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "invalid_token");
}

#[tokio::test]
async fn a_connect_event_with_a_valid_token_reports_a_missing_database() {
    let token = token_for("dev@omoffice.example").await;
    let request = ws_event(
        &format!("/dev?token={token}&org_id=some-org"),
        ws_context("CONNECT", "conn-1"),
    );
    let (status, body) = call(request).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "database_unavailable");
}

/// `$disconnect` always reports success: there is nothing left to reject
/// once the client is already gone, even when removing its row fails.
#[tokio::test]
async fn a_disconnect_event_always_succeeds() {
    let request = ws_event("/dev", ws_context("DISCONNECT", "conn-1"));
    let response = test_app().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_unmatched_http_path_is_still_a_404() {
    let response = test_app().oneshot(get("/no/such/route")).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
