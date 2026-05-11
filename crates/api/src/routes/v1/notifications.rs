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
use serde_json::Value;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/",    get(list_notifications))
        .route("/:id", axum::routing::patch(mark_read))
        .with_state(state)
}

// ── Response shape ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct NotificationResponse {
    pub id:              Uuid,
    pub farmer_id:       Option<Uuid>,
    pub order_id:        Option<Uuid>,
    pub channel:         String,
    pub recipient_email: Option<String>,
    pub event_type:      String,
    pub payload:         Value,
    pub is_sent:         bool,
    pub sent_at:         Option<DateTime<Utc>>,
    pub created_at:      DateTime<Utc>,
}

// ── GET /notifications ─────────────────────────────────────────────────────────
// Farmer views their own notification log — useful for debugging email delivery

#[derive(Debug, Deserialize)]
pub struct ListNotificationsQuery {
    pub page:      Option<i64>,
    pub per_page:  Option<i64>,
    pub is_sent:   Option<bool>,
    pub event_type: Option<String>,
}

async fn list_notifications(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<ListNotificationsQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    let page     = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);
    let offset   = (page - 1) * per_page;

    let rows = sqlx::query!(
        r#"
        SELECT
            id,
            farmer_id,
            order_id,
            channel::text       AS "channel!: String",
            recipient_email,
            event_type,
            payload,
            is_sent,
            sent_at,
            created_at
        FROM notifications
        WHERE farmer_id  = $1
          AND ($2::bool   IS NULL OR is_sent     = $2)
          AND ($3::text   IS NULL OR event_type  = $3)
        ORDER BY created_at DESC
        LIMIT $4 OFFSET $5
        "#,
        farmer_id,
        params.is_sent,
        params.event_type,
        per_page,
        offset,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    let total: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM notifications
        WHERE farmer_id  = $1
          AND ($2::bool  IS NULL OR is_sent    = $2)
          AND ($3::text  IS NULL OR event_type = $3)
        "#,
        farmer_id,
        params.is_sent,
        params.event_type,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    let notifications: Vec<NotificationResponse> = rows
        .into_iter()
        .map(|r| NotificationResponse {
            id:              r.id,
            farmer_id:       r.farmer_id,
            order_id:        r.order_id,
            channel:         r.channel,
            recipient_email: r.recipient_email,
            event_type:      r.event_type,
            payload:         r.payload,
            is_sent:         r.is_sent,
            sent_at:         r.sent_at,
            created_at:      r.created_at,
        })
        .collect();

    Ok(Json(serde_json::json!({
        "data":        notifications,
        "page":        page,
        "per_page":    per_page,
        "total":       total,
        "total_pages": (total as f64 / per_page as f64).ceil() as i64,
    })))
}

// ── PATCH /notifications/:id ───────────────────────────────────────────────────
// Internal use — marks a notification as sent after Resend confirms delivery.
// In production this is called by the mailer crate after a successful send,
// not directly by the farmer. Exposed here for admin visibility + manual retry.

async fn mark_read(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(id):                    Path<Uuid>,
) -> AppResult<StatusCode> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    let rows_affected = sqlx::query!(
        r#"
        UPDATE notifications
        SET is_sent  = true,
            sent_at  = now()
        WHERE id        = $1
          AND farmer_id = $2
          AND is_sent   = false
        "#,
        id,
        farmer_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound(
            "Notification not found, not yours, or already sent".into(),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}