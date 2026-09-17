//! Top-level route table: mounts each module's own routes under its prefix.
//! `/auth/*` and `/chime/*` (EventBridge's webhook, checked by its own
//! shared secret instead -- see `core::chime`) are public; `/rbac/*`,
//! `/meeting/*` and `/map/*` require a bearer token
//! (see [`core::auth::require_bearer`]). The websocket route depends on
//! which transport this build compiled (see the module doc on
//! `core::communication::web_messenger`): a local build adds `GET /ws` --
//! a handshake carries no header to check, so the token rides as
//! `?token=...` instead and the socket authenticates itself on connect,
//! see [`web_messenger::local_web_socket::upgrade_route`] -- while a
//! `lambda`-featured build adds a fallback instead, since
//! `infra/messenger.tf`'s WebSocket API delivers
//! `$connect`/`$default`/`$disconnect` events to this same lambda and they
//! claim no route here a real HTTP request ever would -- see
//! [`web_messenger::web_socket::lambda_entry`].

use axum::{middleware, Extension, Router};
use tower_http::cors::{Any, CorsLayer};

use crate::core::auth;
use crate::core::chime::{self, ChimeState};
use crate::core::communication::web_messenger::{self, Messenger};
use crate::core::data::db::Db;
use crate::core::debug;
use crate::core::media::{self, MediaState};
use crate::core::rbac;
use crate::features::{map, meetings};

pub fn app(db: Db, chime: ChimeState, messenger: Messenger, media: MediaState) -> Router {
    let router = Router::new()
        .nest("/auth", auth::routes())
        .nest("/chime", chime::routes())
        .nest(
            "/rbac",
            rbac::routes().layer(middleware::from_fn(auth::require_bearer)),
        )
        .nest(
            "/meeting",
            meetings::routes().layer(middleware::from_fn(auth::require_bearer)),
        )
        .nest(
            "/map",
            map::routes().layer(middleware::from_fn(auth::require_bearer)),
        )
        .nest(
            "/media",
            media::routes().layer(middleware::from_fn(auth::require_bearer)),
        );

    #[cfg(not(feature = "lambda"))]
    let router = router.route(
        "/ws",
        axum::routing::get(web_messenger::local_web_socket::upgrade_route),
    );
    #[cfg(feature = "lambda")]
    let router = router.fallback(web_messenger::web_socket::lambda_entry);

    router
        .layer(Extension(db))
        .layer(Extension(chime))
        .layer(Extension(messenger))
        .layer(Extension(media))
        // Disable CORS
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(middleware::from_fn(debug::log_requests))
}
