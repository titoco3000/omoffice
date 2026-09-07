use axum::{
    extract::{Extension, Path},
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};

mod auth;
mod chat;
mod core;
mod meetings;
mod messenger;
mod migrate;
mod rbac;
mod schema;
mod users;

use auth::{AuthConfig, Claims, KeySource};
use core::apigw_management::ApiGatewayManagement;
use core::chime::ChimeState;
use core::db::Db;
use messenger::Messenger;
use rbac::{RbacError, RolesConfig};

/// Project-specific role, not native to the RBAC: gates the sample-post demo
/// route below, exactly the example the RBAC was built to support.
const ROLE_POST: &str = "post";

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

/// Body of `POST /private/sample-post/{var}`. `org_id` is the RBAC demo the
/// prompt names explicitly: the caller needs the `"post"` role in that org.
#[derive(Debug, Deserialize)]
struct SamplePost {
    org_id: String,
    name: String,
    male: bool,
}

/// Sample write route: one path parameter plus a JSON body, echoed back with
/// the caller taken from the verified token — gated on the `"post"` role in
/// the org named by the body, the generic [`rbac::require_role`] check the
/// RBAC exists to provide.
async fn private_sample_post(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(var): Path<String>,
    Json(body): Json<SamplePost>,
) -> Result<Json<Value>, RbacError> {
    rbac::require_role(&db, &claims.sub, &body.org_id, ROLE_POST).await?;
    Ok(Json(json!({
        "received": { "var": var, "name": body.name, "male": body.male, "org_id": body.org_id },
        "by": claims.sub,
    })))
}

/// `GET /private/orgs/{org_id}/billing` — a stub, gated on the RBAC-native
/// `billing` role, giving it a concrete (if trivial) use. Real billing data
/// belongs to a project-specific module, not this demo.
async fn private_org_billing(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    rbac::require_role(&db, &claims.sub, &org_id, rbac::roles::BILLING).await?;
    Ok(Json(json!({ "org_id": org_id, "plan": "demo" })))
}

/// `GET /private/invites` — every invite still pending for the caller's own
/// email, org and meeting together. This is what a "pending invite" session
/// is for someone who logged in on their own instead of following a magic
/// link: `rbac.rs` and `meetings.rs` each own one table and one half of the
/// answer, so this is the small aggregate that combines them -- the same
/// role this file already plays for `private_org_billing` above.
async fn private_pending_invites(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, PendingInvitesError> {
    let org_invites = rbac::pending_invites_for_email(&db, &claims.sub).await?;
    let meeting_invites = meetings::pending_invites_for_email(&db, &claims.sub).await?;
    Ok(Json(json!({
        "org_invites": org_invites,
        "meeting_invites": meeting_invites,
    })))
}

/// Wraps whichever of the two domain errors `private_pending_invites` hits
/// first, so it can still use `?` against both `rbac::pending_invites_for_email`
/// and `meetings::pending_invites_for_email` despite them returning
/// different error types.
enum PendingInvitesError {
    Rbac(RbacError),
    Meetings(meetings::MeetingError),
}

impl From<RbacError> for PendingInvitesError {
    fn from(err: RbacError) -> Self {
        Self::Rbac(err)
    }
}

impl From<meetings::MeetingError> for PendingInvitesError {
    fn from(err: meetings::MeetingError) -> Self {
        Self::Meetings(err)
    }
}

impl axum::response::IntoResponse for PendingInvitesError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Rbac(err) => err.into_response(),
            Self::Meetings(err) => err.into_response(),
        }
    }
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

fn app(
    keys: KeySource,
    chime: ChimeState,
    db: Db,
    messenger: Messenger,
    chat_api: ApiGatewayManagement,
) -> Router {
    let auth = AuthConfig::new(keys)
        // Every route under these prefixes needs a valid bearer token.
        .protect(vec!["/private"])
        // No email whitelist: org invites need to provision arbitrary
        // people, and the module doc already says there's no proof of email
        // ownership here anyway. An empty list means `is_allowed` lets
        // everyone through.
        .build();

    // The native RBAC roles plus this project's own -- "post" for the
    // sample-post demo below. `rbac::roles::DEFAULT` (the empty string) is
    // folded in by the RBAC itself, never offered here as a choice.
    let roles = RolesConfig::new().project_roles([ROLE_POST]).build();

    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/auth/request-key/{email}", get(auth::request_key))
        // Not under `/private`: see `chat/mod.rs` for why the websocket
        // handshake can't carry a bearer header, and how this authenticates
        // itself instead.
        .route("/chat/ws/{org_id}", get(chat::ws))
        // The room-less twin: presence only, for an invite's live toast to
        // someone who isn't a member of the org they're being invited into
        // yet. See `chat::ws_presence`.
        .route("/chat/ws", get(chat::ws_presence))
        .route("/private/me", get(private_me))
        .route("/private/invites", get(private_pending_invites))
        .route("/private/sample-post/{var}", post(private_sample_post))
        .route(
            "/private/meetings",
            post(meetings::create).get(meetings::list),
        )
        .route("/private/meetings/{code}/attendees", post(meetings::join))
        .route(
            "/private/orgs",
            post(rbac::create_org_route).get(rbac::list_orgs_route),
        )
        .route(
            "/private/orgs/{org_id}",
            get(rbac::org_detail_route).delete(rbac::delete_org_route),
        )
        .route(
            "/private/orgs/{org_id}/members",
            post(rbac::add_member_route),
        )
        .route(
            "/private/orgs/{org_id}/members/{email}/roles",
            put(rbac::replace_roles_route),
        )
        .route(
            "/private/orgs/{org_id}/members/me",
            delete(rbac::leave_org_route),
        )
        .route("/private/orgs/{org_id}/billing", get(private_org_billing))
        .route(
            "/private/orgs/{org_id}/invites",
            post(rbac::create_invite_route).get(rbac::list_invites_route),
        )
        .route(
            "/private/orgs/{org_id}/invites/{invite_id}",
            delete(rbac::revoke_invite_route),
        )
        .route(
            "/invites/org/{invite_id}/accept",
            post(rbac::accept_invite_route),
        )
        .route(
            "/private/invites/org/{invite_id}/decline",
            post(rbac::decline_invite_route),
        )
        .route(
            "/private/meetings/{code}/invites",
            post(meetings::create_invite_route),
        )
        .route(
            "/private/meetings/{code}/invites/{invite_id}",
            delete(meetings::revoke_invite_route),
        )
        .route(
            "/invites/meetings/{invite_id}/accept",
            post(meetings::accept_invite_route),
        )
        .route(
            "/private/invites/meetings/{invite_id}/decline",
            post(meetings::decline_invite_route),
        )
        // Nothing routes here on purpose: it only exists to catch a
        // WebSocket-API event from `infra/chat.tf`, which never carries a
        // path any real route would claim. A genuinely unmatched HTTP path
        // still gets a plain 404 from it -- see `chat::lambda_entry`.
        .fallback(chat::lambda_entry)
        // The Chime client, the database pool, the messenger, the chat
        // WebSocket-API push client and the project's role list reach their
        // handlers as extensions, so the auth policy stays the router's only
        // `State`.
        .layer(Extension(chime))
        .layer(Extension(db))
        .layer(Extension(messenger))
        .layer(Extension(chat_api))
        .layer(Extension(roles))
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
    // `omoffice-api migrate <status|up|down>` runs the schema tool instead of
    // the server: one binary, and the runner shares the connection code. It
    // prints its own report, so the AWS SDK's INFO chatter is turned down to
    // leave that readable.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let migrating = args.first().is_some_and(|arg| arg == "migrate");

    tracing_subscriber::fmt()
        .with_max_level(if migrating {
            tracing::Level::WARN
        } else {
            tracing::Level::INFO
        })
        .with_target(false)
        .without_time()
        .init();

    if migrating {
        if let Err(message) = migrate::run(&args[1..]).await {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // AWS_LAMBDA_RUNTIME_API is only set by the Lambda execution environment.
    let in_lambda = std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok();
    let keys = KeySource::from_env(in_lambda).await;
    let chime = ChimeState::from_env().await;
    let db = Db::from_env().await;
    let messenger = Messenger::new();
    let chat_api = ApiGatewayManagement::from_env().await;

    if in_lambda {
        return lambda_http::run(app(keys, chime, db, messenger, chat_api)).await;
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("listening on http://localhost:{port}");
    axum::serve(listener, app(keys, chime, db, messenger, chat_api)).await?;
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
        // `app()` no longer whitelists emails -- org invites need to be able
        // to provision arbitrary ones. `TOKEN_TTL` (auth/mod.rs) is 7 days.
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

    /// `test_app` has no database, so the `"post"`-role check (`rbac::require_role`)
    /// can't even run its query -- it reports the same 503 the meeting routes do
    /// rather than reaching DSQL. Exercising the actual role gate (200 with the
    /// role, 403 without) needs `make run dev` against the real dev database.
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

    /// `test_app` has no database either, so the history route says so instead
    /// of reaching DSQL.
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

    /// `create`/`list` both check org membership before touching Chime at
    /// all (`rbac::require_member`), and `test_app` has no database either
    /// -- so an authenticated call reports the same `database_unavailable`
    /// every other DB-backed route does, never reaching the Chime client
    /// (also absent here) to say so instead.
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
        let (status, body) =
            call(post_json("/private/orgs", None, json!({ "name": "acme" }))).await;
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

    /// `test_app` has no database, so an authenticated call gets the same 503
    /// every other DB-backed route reports rather than reaching DSQL.
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

    /// Not under `/private` on purpose -- the magic link has to work with no
    /// bearer token at all (see `rbac::accept_invite_route`'s doc). `test_app`
    /// has no database, so it reaches that far and then reports the same 503
    /// every other DB-backed route does, rather than a 401.
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

    /// Same shape as the org accept route's test above.
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

    /// `test_app` has no database, so the org half of the aggregate is what
    /// reports it -- `private_pending_invites` tries `rbac`'s query first.
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

    // `chat::lambda_entry` is the router's fallback (see `app()`), reached by
    // a WebSocket-API event from `infra/chat.tf`. `lambda_http`'s own event
    // deserialization (JSON -> `ApiGatewayWebsocketProxyRequest` ->
    // `http::Request` carrying a `RequestContext::WebSocket` extension) runs
    // only inside `lambda_http::run`, never inside this test harness, so
    // these build that extension directly -- the same shape `lambda_http`
    // would have produced -- rather than exercising a crate that ships its
    // own test suite for that conversion.
    use lambda_http::aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequestContext;
    use lambda_http::request::RequestContext;

    /// A `requestContext` for a websocket event, built by deserializing the
    /// couple of fields these tests care about -- `ApiGatewayWebsocketProxyRequestContext`
    /// is `#[non_exhaustive]`, so this crate cannot build one as a struct
    /// literal the way `lambda_http` itself does.
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

    /// `test_app` has no database, so a valid token still can't get past the
    /// membership check -- same 503 every other DB-backed route reports.
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

    /// A genuinely unmatched HTTP path (no `RequestContext::WebSocket`
    /// extension) still gets a plain 404 from the fallback, same as before
    /// `chat::lambda_entry` existed.
    #[tokio::test]
    async fn an_unmatched_http_path_is_still_a_404() {
        let response = test_app().oneshot(get("/no/such/route")).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
