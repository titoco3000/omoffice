//! Top-level route table: mounts each module's own routes under its prefix.
//! `/auth/*` is public; `/rbac/*` and `/meeting/*` require a bearer token
//! (see [`core::auth::require_bearer`]). `/ws` is the one route outside
//! those three prefixes: a handshake carries no header to check, so the
//! socket authenticates itself once connected instead -- see
//! [`web_messenger::upgrade_route`].

use axum::{middleware, routing::get, Extension, Router};
use tower_http::cors::{Any, CorsLayer};

use crate::core::auth;
use crate::core::chime::ChimeState;
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::Db;
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
        .route("/ws/{user_id}", get(web_messenger::upgrade_route))
        .layer(Extension(db))
        .layer(Extension(chime))
        .layer(Extension(messenger))
        // Any origin: the API is bearer-token authenticated, not
        // cookie-based, so there is no session for a third-party origin to
        // ride along on. Outermost layer so it also answers the browser's
        // OPTIONS preflight before a request ever reaches `require_bearer`.
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}
