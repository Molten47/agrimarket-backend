use axum::{
    extract::{Path, State},
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
        .route("/",              get(list_stock))
        .route("/:product_slug", get(get_stock).patch(update_stock))
        .with_state(state)
}

// ── Response shape ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StockResponse {
    pub stock_id:            Uuid,
    pub product_id:          Uuid,
    pub product_name:        String,
    pub product_slug:        String,
    pub quantity_available:  Decimal,
    pub quantity_reserved:   Decimal,
    pub low_stock_threshold: Decimal,
    pub stock_status:        String,
    pub updated_at:          DateTime<Utc>,
}

// ── GET /stock ─────────────────────────────────────────────────────────────────
// Farmer's full stock overview — all their active products with current levels

async fn list_stock(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
) -> AppResult<Json<serde_json::Value>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    let rows = sqlx::query!(
        r#"
        SELECT
            s.id                  AS stock_id,
            s.product_id,
            p.name                AS product_name,
            p.slug                AS product_slug,
            s.quantity_available,
            s.quantity_reserved,
            s.low_stock_threshold,
            s.stock_status::text  AS "stock_status!: String",
            s.updated_at
        FROM stock s
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id  = $1
          AND p.is_active  = true
          AND p.is_deleted = false
        ORDER BY s.stock_status, p.name
        "#,
        farmer_id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    let stock: Vec<StockResponse> = rows
        .into_iter()
        .map(|r| StockResponse {
            stock_id:            r.stock_id,
            product_id:          r.product_id,
            product_name:        r.product_name,
            product_slug:        r.product_slug,
            quantity_available:  r.quantity_available,
            quantity_reserved:   r.quantity_reserved,
            low_stock_threshold: r.low_stock_threshold,
            stock_status:        r.stock_status,
            updated_at:          r.updated_at,
        })
        .collect();

    // Summary counts — useful for the farmer dashboard
    let total          = stock.len();
    let out_of_stock   = stock.iter().filter(|s| s.stock_status == "out_of_stock").count();
    let low_stock      = stock.iter().filter(|s| s.stock_status == "low_stock").count();
    let in_stock       = stock.iter().filter(|s| s.stock_status == "in_stock").count();

    Ok(Json(serde_json::json!({
        "data": stock,
        "summary": {
            "total":        total,
            "in_stock":     in_stock,
            "low_stock":    low_stock,
            "out_of_stock": out_of_stock,
        }
    })))
}

// ── GET /stock/:product_slug ───────────────────────────────────────────────────

async fn get_stock(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(product_slug):          Path<String>,
) -> AppResult<Json<StockResponse>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    let row = sqlx::query!(
        r#"
        SELECT
            s.id                  AS stock_id,
            s.product_id,
            p.name                AS product_name,
            p.slug                AS product_slug,
            s.quantity_available,
            s.quantity_reserved,
            s.low_stock_threshold,
            s.stock_status::text  AS "stock_status!: String",
            s.updated_at
        FROM stock s
        JOIN products p ON p.id = s.product_id
        WHERE p.slug       = $1
          AND p.farmer_id  = $2
          AND p.is_active  = true
          AND p.is_deleted = false
        "#,
        product_slug,
        farmer_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Stock record not found".into()))?;

    Ok(Json(StockResponse {
        stock_id:            row.stock_id,
        product_id:          row.product_id,
        product_name:        row.product_name,
        product_slug:        row.product_slug,
        quantity_available:  row.quantity_available,
        quantity_reserved:   row.quantity_reserved,
        low_stock_threshold: row.low_stock_threshold,
        stock_status:        row.stock_status,
        updated_at:          row.updated_at,
    }))
}

// ── PATCH /stock/:product_slug ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct UpdateStockInput {
    /// New absolute quantity available — replaces current value
    quantity_available:  Option<Decimal>,
    /// Update the threshold that triggers low_stock status
    low_stock_threshold: Option<Decimal>,
}

async fn update_stock(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(product_slug):          Path<String>,
    Json(input):                 Json<UpdateStockInput>,
) -> AppResult<(StatusCode, Json<StockResponse>)> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    if input.quantity_available.is_none() && input.low_stock_threshold.is_none() {
        return Err(AppError::Validation(
            "Provide at least one of: quantity_available, low_stock_threshold".into(),
        ));
    }

    // Fetch current stock to merge with incoming partial update
    let current = sqlx::query!(
        r#"
        SELECT
            s.id,
            s.quantity_available,
            s.quantity_reserved,
            s.low_stock_threshold
        FROM stock s
        JOIN products p ON p.id = s.product_id
        WHERE p.slug       = $1
          AND p.farmer_id  = $2
          AND p.is_active  = true
          AND p.is_deleted = false
        "#,
        product_slug,
        farmer_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Stock record not found".into()))?;

    // Merge — use incoming value or fall back to current
    let new_quantity  = input.quantity_available
        .unwrap_or(current.quantity_available);
    let new_threshold = input.low_stock_threshold
        .unwrap_or(current.low_stock_threshold);

    // Recompute stock_status from new values — single source of truth
    // quantity_reserved is subtracted: only truly free stock counts
    let free_quantity = new_quantity - current.quantity_reserved;
    let new_status = if free_quantity <= Decimal::ZERO {
        "out_of_stock"
    } else if free_quantity <= new_threshold {
        "low_stock"
    } else {
        "in_stock"
    };

    // Update and return the fresh row in one query
    let updated = sqlx::query!(
        r#"
        UPDATE stock
        SET
            quantity_available  = $1,
            low_stock_threshold = $2,
            stock_status        = $3::stock_status,
            updated_at          = now()
        WHERE id = $4
        RETURNING
            id                  AS stock_id,
            product_id,
            quantity_available,
            quantity_reserved,
            low_stock_threshold,
            stock_status::text  AS "stock_status!: String",
            updated_at
        "#,
        new_quantity  as Decimal,
        new_threshold as Decimal,
        new_status    as &str,
        current.id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((
        StatusCode::OK,
        Json(StockResponse {
            stock_id:            updated.stock_id,
            product_id:          updated.product_id,
            product_name:        product_slug.clone(), // slug used as name proxy here
            product_slug,
            quantity_available:  updated.quantity_available,
            quantity_reserved:   updated.quantity_reserved,
            low_stock_threshold: updated.low_stock_threshold,
            stock_status:        updated.stock_status,
            updated_at:          updated.updated_at,
        }),
    ))
}