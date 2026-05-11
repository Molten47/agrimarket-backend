use axum::{routing::get, Json, Router};
use serde_json::json;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/", get(root))
}

async fn root() -> Json<serde_json::Value> {
    Json(json!({ "service": "AgriMarket API", "version": "1.0.0" }))
}

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}
