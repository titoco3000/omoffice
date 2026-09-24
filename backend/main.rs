//! Entry point: `migrate <status|up|down|generate>` runs the migration
//! tool; anything else builds the shared state, wires each feature's
//! channels and events, and hands `router::app` to [`serve`]. Which of the
//! two ways `serve` runs it -- a local TCP listener, or `lambda_http::run`
//! -- is chosen at compile time by the `lambda` feature, the same one that
//! picks `core::communication::web_messenger`'s transport; see that
//! module's doc for why this is a `#[cfg]`, not a runtime check. This file
//! is deliberately thin -- argument parsing and wiring only, calling
//! straight into the `omoffice_api` library crate under `lib/` for
//! everything else.

use omoffice_api::core::chime::ChimeState;
use omoffice_api::core::communication::web_messenger::Messenger;
use omoffice_api::core::data::db::Db;
use omoffice_api::core::data::orm::migrate;
use omoffice_api::core::media::MediaState;
use omoffice_api::{core, features, router};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let migrating = args.first().is_some_and(|arg| arg == "migrate");

    tracing_subscriber::fmt()
        .with_max_level(if migrating {
            tracing::Level::WARN
        } else {
            tracing::Level::INFO
        })
        .with_target(false)
        .init();

    if migrating {
        if let Err(message) = migrate::run(&args[1..]).await {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
        return;
    }

    let db = Db::from_env().await;
    let chime = ChimeState::from_env().await;
    let messenger = Messenger::new(db.clone()).await;
    let media = MediaState::from_env().await;
    let map_state = features::map::MapState::hydrate(&db).await;

    // Each feature wires its own channels/events before the router is
    // built, so registering one is never silently forgotten at startup.
    core::rbac::register(&messenger, db.clone());
    features::map::register(&messenger, db.clone(), map_state.clone());
    features::meetings::register(&messenger, db.clone());
    features::meetings::rooms::register(db.clone(), &chime);
    features::meetings::chat::register(&messenger, db.clone());
    features::meetings::controls::register(&messenger, db.clone());
    features::meetings::access_requests::register(&messenger, db.clone());

    let app = router::app(db.clone(), chime, messenger, media, map_state.clone());
    serve(app, db, map_state).await;
}

/// Code that runs on lambda
#[cfg(feature = "lambda")]
async fn serve(app: axum::Router, _db: Db, _map_state: features::map::MapState) {
    if let Err(err) = lambda_http::run(app).await {
        eprintln!("lambda runtime error: {err}");
        std::process::exit(1);
    }
}

/// Code that runs locally
#[cfg(not(feature = "lambda"))]
async fn serve(app: axum::Router, db: Db, map_state: features::map::MapState) {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("port is free");
    tracing::info!("listening on http://localhost:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(db, map_state))
        .await
        .expect("server crashed");
}

/// Waits for Ctrl-C or SIGTERM (what ECS sends on task stop), then flushes
/// every in-memory map position to the database before returning -- see
/// `features::map::flush_all`. `axum::serve`'s graceful shutdown waits for
/// this future before it stops accepting/draining connections, so this must
/// stay well under the ECS task definition's `stopTimeout`.
#[cfg(not(feature = "lambda"))]
async fn shutdown_signal(db: Db, map_state: features::map::MapState) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl_c handler");
    };
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install sigterm handler")
            .recv()
            .await;
    };
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received, flushing map positions");
    features::map::flush_all(&db, &map_state).await;
}
