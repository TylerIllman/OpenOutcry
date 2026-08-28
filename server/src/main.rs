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
mod replay;
mod state;
mod translate;
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

    let app_state = AppState::new();

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "web/dist".to_string());
    let index = format!("{static_dir}/index.html");

    let app = Router::new()
        .route("/api/sessions", post(http::create_session))
        .route("/api/sessions/{code}", get(http::get_session))
        .route("/api/sessions/{code}/join", post(http::join_session))
        .route("/api/sessions/{code}/export.csv", get(http::export_csv))
        .route("/api/sessions/{code}/verify", get(http::verify))
        .route("/ws", get(ws::ws_handler))
        // Anything else is the SPA. The fallback to index.html is what makes
        // /host/ABC123 work on a hard refresh — and the QR code points straight
        // at /join/ABC123, so this is the path most people arrive on.
        //
        // `fallback`, not `not_found_service`: the latter serves index.html but
        // forces a 404 status. Browsers render the body anyway, so the app
        // appears to work while every deep link reports as missing.
        // Hashed build output is served on its own, with no SPA fallback. A
        // missing chunk should be a 404, not index.html served as JavaScript —
        // that turns a stale deploy into an inscrutable MIME type error.
        .nest_service("/assets", ServeDir::new(format!("{static_dir}/assets")))
        .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(&index)))
        .layer(TraceLayer::new_for_http())
        .with_state(app_state.clone());

    let port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080u16);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    // Sessions live in memory, so abandoned ones would otherwise accumulate for
    // as long as the process runs.
    let sweeper = app_state.clone();
    tokio::spawn(async move {
        let ttl_ms: i64 = std::env::var("SESSION_TTL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6 * 60 * 60 * 1000); // six hours
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            tick.tick().await;
            let dropped = sweeper.evict_idle(ttl_ms).await;
            if dropped > 0 {
                tracing::info!("evicted {dropped} idle session(s)");
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    tracing::info!("listening on http://{addr} (serving {static_dir})");

    axum::serve(listener, app).await.expect("serve");
}
