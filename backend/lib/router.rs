//! Top-level route table: mounts each module's own routes under its prefix.
//! `/auth/*` is public; `/rbac/*` and `/meeting/*` require a bearer token
//! (see [`core::auth::require_bearer`]). `/ws` is the one route outside
//! those three prefixes: a handshake carries no header to check, so the
//! token rides as `?token=...` instead and the socket authenticates itself
//! on connect -- see [`web_messenger::upgrade_route`]. The fallback exists
//! only for a deployed Lambda: `infra/chat.tf`'s WebSocket API delivers
//! `$connect`/`$default`/`$disconnect` events to this same lambda, and they
//! claim no route here a real HTTP request ever would -- see
//! [`web_messenger::lambda_entry`].

use axum::{middleware, routing::get, Extension, Router};
use tower_http::cors::{Any, CorsLayer};

use crate::core::auth;
use crate::core::chime::ChimeState;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::Db;
use crate::core::debug;
use crate::core::rbac;
use crate::features::meetings;

pub fn app(db: Db, chime: ChimeState, messenger: Messenger) -> Router {
    Router::new()
        .nest("/auth", auth::routes())
        .nest(
            "/rbac",
            rbac::routes().layer(middleware::from_fn(auth::require_bearer)),
        )
        .nest(
            "/meeting",
            meetings::routes().layer(middleware::from_fn(auth::require_bearer)),
        )
        .route("/ws", get(web_messenger::upgrade_route))
        .fallback(web_messenger::lambda_entry)
        .layer(Extension(db))
        .layer(Extension(chime))
        .layer(Extension(messenger))
        // Disable CORS
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(middleware::from_fn(debug::log_requests))
}
