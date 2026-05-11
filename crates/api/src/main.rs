use axum::{Router, extract::Extension};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
    compression::CompressionLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use mailer::MailerService;

mod routes;
mod state;
mod middleware;

use app_core::{config::AppConfig, db, redis as cache};
use auth::service::AuthService;
use ws::{WsBroadcaster, ws_handler};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── 1. Tracing ────────────────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "agrimarket=debug,tower_http=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // ── 2. Config ─────────────────────────────────────────────────────────
    let config = AppConfig::load()?;
    tracing::info!("Environment: {:?}", config.environment);

    let mailer = MailerService::new(
    &config.resend_api_key,
    &config.email_from,
    &config.email_from_name,
);

    // ── 3. Database ───────────────────────────────────────────────────────
    let pool = db::create_pool(&config.database_url, config.db_max_conns).await?;

    // ── 4. Redis ──────────────────────────────────────────────────────────
    let redis = cache::create_client(&config.redis_url).await?;

    // ── 5. Auth service ───────────────────────────────────────────────────
    let auth = AuthService::from_config(&config)?;

    // ── 6. WebSocket broadcaster ──────────────────────────────────────────
    let broadcaster = WsBroadcaster::new();

    // ── 7. Shared state ───────────────────────────────────────────────────
    let state = Arc::new(AppState::new(
        config.clone(), pool, redis, auth, broadcaster.clone(), mailer,
    ));

    // ── 8. CORS ───────────────────────────────────────────────────────────
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // ── 9. Router ─────────────────────────────────────────────────────────
    let app = Router::new()
        .merge(routes::health::router())
        .nest("/api/v1", routes::v1::router(state.clone()))
        // WebSocket endpoint — outside /api/v1, no versioning needed
        .route("/ws", axum::routing::get(ws_handler))
        .layer(Extension(broadcaster))          // broadcaster injected via Extension
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new());

    // ── 10. Listen ────────────────────────────────────────────────────────
    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    tracing::info!("Listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}