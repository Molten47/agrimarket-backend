use axum::{routing::post, Router};
use std::sync::Arc;
use crate::state::AppState;

pub mod auth;
pub mod products;
pub mod categories;
pub mod stock;
pub mod cart;
pub mod orders;
pub mod payments;
pub mod tracking;
pub mod notifications;
pub mod upload;
pub mod analytics;
pub mod bookkeeping;






pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .nest("/auth",          auth::router(state.clone()))
        .nest("/products",      products::router(state.clone()))
        .nest("/categories",    categories::router(state.clone()))
        .nest("/stock",         stock::router(state.clone()))
        .nest("/cart",          cart::router(state.clone()))
        .nest("/orders",        orders::router(state.clone()))
        .nest("/payments",      payments::router(state.clone()))
        .nest("/tracking",      tracking::router(state.clone()))
        .nest("/notifications", notifications::router(state.clone()))
        .nest("/upload",        upload::router(state.clone()))
        .nest("/analytics", analytics::router(state.clone()))
        .nest("/bookkeeping", bookkeeping::router(state.clone()))
}


// inside your v1 router function:
