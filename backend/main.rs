//! Entry point: `migrate generate` runs the schema tool; anything else
//! builds the shared state, wires each feature's channels and events, and
//! serves `router::app`. This file is deliberately thin -- argument parsing
//! and wiring only, calling straight into the `omoffice_api` library crate
//! under `lib/` for everything else.

use omoffice_api::core::chime::ChimeState;
use omoffice_api::core::communication::web_messenger::Messenger;
use omoffice_api::core::data::db::Db;
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
        if let Err(message) = run_migrate(&args[1..]) {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
        return;
    }

    let db = Db::from_env().await;
    let chime = ChimeState::from_env().await;
    let messenger = Messenger::new();

    // Each feature wires its own channels/events before the router is
    // built, so registering one is never silently forgotten at startup.
    core::rbac::register(&messenger);
    features::meetings::rooms::register(db.clone(), &chime);
    features::meetings::chat::register(&messenger, db.clone());

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("port is free");
    tracing::info!("listening on http://localhost:{port}");
    axum::serve(listener, router::app(db, chime, messenger))
        .await
        .expect("server crashed");
}

/// `cargo run -- migrate generate` -- the only subcommand so far, wired
/// straight to `core::data::orm::generate`. `migrate up`/`down`/`status`
/// come back once there's a runner to apply what it writes.
fn run_migrate(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("generate") => core::data::orm::generate(),
        Some(other) => Err(format!("unknown migrate subcommand: {other}")),
        None => Err("usage: migrate generate".to_string()),
    }
}
