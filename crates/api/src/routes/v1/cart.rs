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
use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/:session_key",                          get(get_cart).delete(clear_cart))
        .route("/:session_key/items",                    axum::routing::post(add_item))
        .route("/:session_key/items/:product_id",        axum::routing::patch(update_item)
                                                            .delete(remove_item))
        .with_state(state)
}

// ── Response shapes ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CartItemResponse {
    pub product_id:    Uuid,
    pub product_name:  String,
    pub product_slug:  String,
    pub farm_name:     String,
    pub unit:          String,
    pub quantity:      Decimal,
    pub price_per_unit: Decimal,
    pub line_total:    Decimal,
    pub stock_status:  String,
}

#[derive(Debug, Serialize)]
pub struct CartResponse {
    pub cart_id:     Uuid,
    pub session_key: String,
    pub items:       Vec<CartItemResponse>,
    pub total:       Decimal,
    pub item_count:  usize,
    pub expires_at:  DateTime<Utc>,
}

// ── GET /cart/:session_key ─────────────────────────────────────────────────────

async fn get_cart(
    State(state):    State<Arc<AppState>>,
    Path(session_key): Path<String>,
) -> AppResult<Json<CartResponse>> {
    // Upsert cart — creates it if it doesn't exist yet (7 day TTL)
    let cart = sqlx::query!(
        r#"
        INSERT INTO cart (session_key, expires_at)
        VALUES ($1, now() + interval '7 days')
        ON CONFLICT (session_key) DO UPDATE
            SET expires_at = GREATEST(cart.expires_at, now() + interval '7 days')
        RETURNING id, session_key, expires_at
        "#,
        session_key,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    // Fetch items with a full JOIN — price and stock always live
    let rows = sqlx::query!(
        r#"
        SELECT
            ci.product_id,
            p.name              AS product_name,
            p.slug              AS product_slug,
            f.farm_name,
            p.unit,
            ci.quantity,
            p.price_per_unit,
            s.stock_status::text AS "stock_status!: String"
        FROM cart_items ci
        JOIN products p ON p.id = ci.product_id
        JOIN farmers  f ON f.id = p.farmer_id
        JOIN stock    s ON s.product_id = p.id
        WHERE ci.cart_id = $1
        ORDER BY ci.added_at
        "#,
        cart.id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    let items: Vec<CartItemResponse> = rows
        .into_iter()
        .map(|r| {
            let line_total = r.quantity * r.price_per_unit;
            CartItemResponse {
                product_id:    r.product_id,
                product_name:  r.product_name,
                product_slug:  r.product_slug,
                farm_name:     r.farm_name,
                unit:          r.unit,
                quantity:      r.quantity,
                price_per_unit: r.price_per_unit,
                line_total,
                stock_status:  r.stock_status,
            }
        })
        .collect();

    let total = items.iter().map(|i| i.line_total).sum();
    let item_count = items.len();

    Ok(Json(CartResponse {
        cart_id:     cart.id,
        session_key: cart.session_key,
        items,
        total,
        item_count,
        expires_at:  cart.expires_at,
    }))
}

// ── POST /cart/:session_key/items ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AddItemInput {
    product_id: Uuid,
    quantity:   Decimal,
}

async fn add_item(
    State(state):      State<Arc<AppState>>,
    Path(session_key): Path<String>,
    Json(input):       Json<AddItemInput>,
) -> AppResult<StatusCode> {
    if input.quantity <= Decimal::ZERO {
        return Err(AppError::Validation("Quantity must be greater than zero".into()));
    }

    // Ensure cart exists
    let cart = sqlx::query!(
        r#"
        INSERT INTO cart (session_key, expires_at)
        VALUES ($1, now() + interval '7 days')
        ON CONFLICT (session_key) DO UPDATE
            SET expires_at = GREATEST(cart.expires_at, now() + interval '7 days')
        RETURNING id
        "#,
        session_key,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    // Check stock availability before adding
    let stock = sqlx::query!(
        r#"
        SELECT
            quantity_available - quantity_reserved AS "free_quantity!: Decimal",
            stock_status::text                     AS "stock_status!: String"
        FROM stock
        WHERE product_id = $1
        "#,
        input.product_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Product not found".into()))?;

    if stock.stock_status == "out_of_stock" {
        return Err(AppError::Conflict("Product is out of stock".into()));
    }

    if input.quantity > stock.free_quantity {
        return Err(AppError::Validation(format!(
            "Only {:.3} units available",
            stock.free_quantity
        )));
    }

    // Upsert cart item — if already in cart, add quantities
    sqlx::query!(
        r#"
        INSERT INTO cart_items (cart_id, product_id, quantity)
        VALUES ($1, $2, $3)
        ON CONFLICT (cart_id, product_id) DO UPDATE
            SET quantity = cart_items.quantity + $3
        "#,
        cart.id,
        input.product_id,
        input.quantity as Decimal,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok(StatusCode::NO_CONTENT)
}

// ── PATCH /cart/:session_key/items/:product_id ────────────────────────────────

#[derive(Debug, Deserialize)]
struct UpdateItemInput {
    quantity: Decimal,
}

async fn update_item(
    State(state):                   State<Arc<AppState>>,
    Path((session_key, product_id)): Path<(String, Uuid)>,
    Json(input):                    Json<UpdateItemInput>,
) -> AppResult<StatusCode> {
    if input.quantity <= Decimal::ZERO {
        return Err(AppError::Validation("Quantity must be greater than zero".into()));
    }

    // Verify stock can cover the new quantity
    let stock = sqlx::query!(
        r#"
        SELECT quantity_available - quantity_reserved AS "free_quantity!: Decimal"
        FROM stock
        WHERE product_id = $1
        "#,
        product_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Product not found".into()))?;

    if input.quantity > stock.free_quantity {
        return Err(AppError::Validation(format!(
            "Only {:.3} units available",
            stock.free_quantity
        )));
    }

    let rows_affected = sqlx::query!(
        r#"
        UPDATE cart_items
        SET quantity = $1
        WHERE cart_id  = (SELECT id FROM cart WHERE session_key = $2)
          AND product_id = $3
        "#,
        input.quantity as Decimal,
        session_key,
        product_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound("Item not in cart".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /cart/:session_key/items/:product_id ───────────────────────────────

async fn remove_item(
    State(state):                    State<Arc<AppState>>,
    Path((session_key, product_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    sqlx::query!(
        r#"
        DELETE FROM cart_items
        WHERE cart_id   = (SELECT id FROM cart WHERE session_key = $1)
          AND product_id = $2
        "#,
        session_key,
        product_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /cart/:session_key ─────────────────────────────────────────────────

async fn clear_cart(
    State(state):      State<Arc<AppState>>,
    Path(session_key): Path<String>,
) -> AppResult<StatusCode> {
    // Deletes cart_items via CASCADE, then the cart row itself
    sqlx::query!(
        "DELETE FROM cart WHERE session_key = $1",
        session_key,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok(StatusCode::NO_CONTENT)
}