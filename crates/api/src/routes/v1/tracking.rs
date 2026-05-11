use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/:order_id", get(get_tracking).post(add_tracking_event))
        .with_state(state)
}

// ── Response shape ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TrackingEventResponse {
    pub id:             Uuid,
    pub order_id:       Uuid,
    pub status:         String,
    pub location_label: Option<String>,
    pub lat:            Option<Decimal>,
    pub lng:            Option<Decimal>,
    pub event_time:     DateTime<Utc>,
}

// ── GET /tracking/:order_id ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GetTrackingQuery {
    /// Guest proves order ownership with their order_key
    pub order_key: Option<String>,
}

async fn get_tracking(
    State(state):   State<Arc<AppState>>,
    Path(order_id): Path<Uuid>,
    Query(params):  Query<GetTrackingQuery>,
) -> AppResult<Json<Vec<TrackingEventResponse>>> {
    // Verify order exists and caller owns it via order_key
    let order = sqlx::query!(
        "SELECT order_key FROM orders WHERE id = $1",
        order_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Order not found".into()))?;

    match &params.order_key {
        Some(key) if *key == order.order_key => {}
        _ => return Err(AppError::Unauthorised),
    }

    let events = sqlx::query!(
        r#"
        SELECT
            id,
            order_id,
            status,
            location_label,
            lat,
            lng,
            event_time
        FROM tracking
        WHERE order_id = $1
        ORDER BY event_time ASC
        "#,
        order_id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| TrackingEventResponse {
        id:             r.id,
        order_id:       r.order_id,
        status:         r.status,
        location_label: r.location_label,
        lat:            r.lat,
        lng:            r.lng,
        event_time:     r.event_time,
    })
    .collect();

    Ok(Json(events))
}

// ── POST /tracking/:order_id ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddTrackingInput {
    pub status:         String,
    pub location_label: Option<String>,
    /// Optional GPS coordinates — useful for live delivery tracking on the map
    pub lat:            Option<Decimal>,
    pub lng:            Option<Decimal>,
}

async fn add_tracking_event(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(order_id):              Path<Uuid>,
    Json(input):                 Json<AddTrackingInput>,
) -> AppResult<(StatusCode, Json<TrackingEventResponse>)> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    if input.status.trim().is_empty() {
        return Err(AppError::Validation("Status cannot be empty".into()));
    }

    // lat and lng must both be present or both absent
    if input.lat.is_some() != input.lng.is_some() {
        return Err(AppError::Validation(
            "Provide both lat and lng, or neither".into(),
        ));
    }

    // Verify the order belongs to this farmer via stock → products chain
    let order_exists = sqlx::query!(
        r#"
        SELECT o.id
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE o.id        = $1
          AND p.farmer_id = $2
        "#,
        order_id,
        farmer_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?;

    if order_exists.is_none() {
        return Err(AppError::NotFound("Order not found or not yours".into()));
    }

    // Append-only — tracking events are never updated or deleted
    let event = sqlx::query!(
        r#"
        INSERT INTO tracking (order_id, status, location_label, lat, lng)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, order_id, status, location_label, lat, lng, event_time
        "#,
        order_id,
        input.status.trim(),
        input.location_label,
        input.lat  as Option<Decimal>,
        input.lng  as Option<Decimal>,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((
        StatusCode::CREATED,
        Json(TrackingEventResponse {
            id:             event.id,
            order_id:       event.order_id,
            status:         event.status,
            location_label: event.location_label,
            lat:            event.lat,
            lng:            event.lng,
            event_time:     event.event_time,
        }),
    ))
}