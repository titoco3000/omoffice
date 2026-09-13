//! Entry point: `migrate <status|up|down|generate>` runs the migration
//! tool; anything else builds the shared state, wires each feature's
//! channels and events, and serves `router::app` -- over a local TCP
//! listener normally, or straight to `lambda_http::run` when
//! `AWS_LAMBDA_RUNTIME_API` says the process is running in Lambda. This file
//! is deliberately thin -- argument parsing and wiring only, calling
//! straight into the `omoffice_api` library crate under `lib/` for
//! everything else.

use omoffice_api::core::chime::ChimeState;
use omoffice_api::core::communication::web_messenger::Messenger;
use omoffice_api::core::data::db::Db;
use omoffice_api::core::data::orm::migrate;
use omoffice_api::{core, features, router};

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
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
        return Ok(());
    }

    let db = Db::from_env().await;
    let chime = ChimeState::from_env().await;
    let messenger = Messenger::new(db.clone()).await;

    // Each feature wires its own channels/events before the router is
    // built, so registering one is never silently forgotten at startup.
    core::rbac::register(&messenger, db.clone());
    features::meetings::register(&messenger, db.clone());
    features::meetings::rooms::register(db.clone(), &chime);
    features::meetings::chat::register(&messenger, db.clone());

    let app = router::app(db, chime, messenger.clone());

    // AWS_LAMBDA_RUNTIME_API is only set by the Lambda execution environment.
    if std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok() {
        return lambda_http::run(app).await;
    }

    // Sockets from an earlier local run cannot outlive its process, but
    // their `ws_connections` rows can -- see `Messenger::clear_local_connections`.
    messenger.clear_local_connections().await;

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("port is free");
    tracing::info!("listening on http://localhost:{port}");
    axum::serve(listener, app).await.expect("server crashed");
    Ok(())
}
