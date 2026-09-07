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
mod migrate;
mod presence;
mod rbac;
mod schema;
mod users;

use auth::{AuthConfig, Claims, KeySource};
use core::apigw_management::ApiGatewayManagement;
use core::chime::ChimeState;
use core::db::Db;
use core::messenger::Messenger;
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

/// Sample write route, gated on the `"post"` role in the org named by the
/// body — the example [`rbac::require_role`] exists to demo.
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
/// `billing` role. Real billing data belongs to a project-specific module.
async fn private_org_billing(
    Extension(db): Extension<Db>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, RbacError> {
    rbac::require_role(&db, &claims.sub, &org_id, rbac::roles::BILLING).await?;
    Ok(Json(json!({ "org_id": org_id, "plan": "demo" })))
}

/// `GET /private/invites` — every org and meeting invite still pending for
/// the caller, for someone who logged in directly instead of a magic link.
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

/// Wraps whichever of the two domain errors `private_pending_invites` hits,
/// so `?` works against both despite their different error types.
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

/// Wide open: no cookies, so the bearer token alone grants access and any
/// origin may call it. Narrow this if the API ever trusts the browser's origin.
fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

fn app(keys: KeySource, chime: ChimeState, db: Db, messenger: Messenger) -> Router {
    let auth = AuthConfig::new(keys)
        // Every route under these prefixes needs a valid bearer token.
        .protect(vec!["/private"])
        // No whitelist: org invites need to provision arbitrary emails.
        .build();

    // "post" is this project's only role beyond the RBAC's native three.
    let roles = RolesConfig::new().project_roles([ROLE_POST]).build();

    // Every channel the messenger will route an incoming frame to. A
    // feature owns a channel name and a handler; nothing else about
    // messaging is wired here.
    chat::register(&messenger);
    presence::register(&messenger);

    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/auth/request-key/{email}", get(auth::request_key))
        // The one socket a browser opens, carrying every channel. Not under
        // `/private`: a handshake has no header, so it authenticates itself
        // with `?token=...` — see `core::messenger::ws`.
        .route("/ws", get(core::messenger::ws))
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
        // Catches a WebSocket-API event, which claims no real route's path.
        .fallback(core::messenger::lambda_entry)
        // Extensions, not State, so the auth policy stays the router's only State.
        .layer(Extension(chime))
        .layer(Extension(db))
        .layer(Extension(messenger))
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
    // `migrate` runs the schema tool instead of the server, and quiets AWS
    // SDK chatter so its own report stays readable.
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
    let messenger = Messenger::new(db.clone(), ApiGatewayManagement::from_env().await);

    if in_lambda {
        return lambda_http::run(app(keys, chime, db, messenger)).await;
    }

    // Sockets from an earlier local run cannot outlive its process, but
    // their rows can -- see `Messenger::clear_local_connections`.
    messenger.clear_local_connections().await;

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("listening on http://localhost:{port}");
    axum::serve(listener, app(keys, chime, db, messenger)).await?;
    Ok(())
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
