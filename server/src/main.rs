//! Open Outcry server.
//!
//! One process serves everything: the built React bundle, the REST endpoints and
//! the WebSocket, all on one origin. That is deliberate — no CORS, no second
//! deploy, no WS URL to configure per environment.
//!
//! State lives in memory, so this must run as exactly one instance. In
//! `fly.toml` that means `auto_stop_machines = false` and
//! `min_machines_running = 1`. Never zero, never two.

mod db;
mod http;
mod protocol;
mod state;
mod ws;

use axum::Router;
use axum::routing::{get, post};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=debug,tower_http=debug".into()),
        )
        .init();

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "web/dist".to_string());
    let index = format!("{static_dir}/index.html");

    let app = Router::new()
        .route("/api/sessions", post(http::create_session))
        .route("/api/sessions/{code}", get(http::get_session))
        .route("/api/sessions/{code}/join", post(http::join_session))
        .route("/api/sessions/{code}/export.csv", get(http::export_csv))
        .route("/ws", get(ws::ws_handler))
        // Anything else is the SPA. The fallback to index.html is what makes
        // /host/ABC123 work on a hard refresh.
        .fallback_service(ServeDir::new(&static_dir).not_found_service(ServeFile::new(&index)))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState::new());

    let port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080u16);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    tracing::info!("listening on http://{addr} (serving {static_dir})");

    axum::serve(listener, app).await.expect("serve");
}
