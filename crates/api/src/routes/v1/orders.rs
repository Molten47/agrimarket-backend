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
use ws::message::WsEvent;
use mailer::templates::EmailTemplate;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/",                 get(list_orders).post(place_order))
        .route("/:order_id",        get(get_order))
        .route("/:order_id/status", axum::routing::patch(update_order_status))
        .with_state(state)
}
// ── Response shapes ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OrderItemResponse {
    pub product_id:     Uuid,
    pub product_name:   String,
    pub product_slug:   String,
    pub quantity:       Decimal,
    pub unit_price_gbp: Decimal,
    pub subtotal_gbp:   Decimal,
}

#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub id:                Uuid,
    pub order_key:         String,
    pub order_status:      String,
    pub payment_status:    String,
    pub guest_email:       String,
    pub guest_phone:       Option<String>,
    pub delivery_address:  String,
    pub delivery_county:   String,
    pub delivery_postcode: String,
    pub total_amount_gbp:  Decimal,
    pub items:             Vec<OrderItemResponse>,
    pub placed_at:         DateTime<Utc>,
}

// ── POST /orders ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PlaceOrderInput {
    pub order_key:         String,
    pub session_key:       String,
    pub guest_email:       String,
    pub guest_phone:       Option<String>,
    pub delivery_address:  String,
    pub delivery_county:   String,
    pub delivery_postcode: String,
}

async fn place_order(
    State(state): State<Arc<AppState>>,
    Json(input):  Json<PlaceOrderInput>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    // ── Idempotency ────────────────────────────────────────────────────────
    let existing = sqlx::query!(
        "SELECT id, order_key FROM orders WHERE order_key = $1",
        input.order_key,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?;

    if let Some(order) = existing {
        return Ok((StatusCode::OK, Json(serde_json::json!({
            "id":        order.id,
            "order_key": order.order_key,
            "message":   "Order already exists",
        }))));
    }

    // ── Fetch cart ─────────────────────────────────────────────────────────
    let cart = sqlx::query!(
        "SELECT id FROM cart WHERE session_key = $1",
        input.session_key,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Cart not found".into()))?;

    // ── Fetch cart items with live stock check ─────────────────────────────
    let cart_items = sqlx::query!(
        r#"
        SELECT
            ci.product_id,
            ci.quantity,
            p.name            AS product_name,
            p.price_per_unit,
            s.id              AS stock_id,
            s.quantity_available - s.quantity_reserved
                              AS "free_quantity!: Decimal"
        FROM cart_items ci
        JOIN products p ON p.id = ci.product_id
        JOIN stock    s ON s.product_id = ci.product_id
        WHERE ci.cart_id = $1
        "#,
        cart.id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    if cart_items.is_empty() {
        return Err(AppError::Validation("Cart is empty".into()));
    }

    for item in &cart_items {
        if item.quantity > item.free_quantity {
            return Err(AppError::Conflict(format!(
                "'{}' has insufficient stock ({:.3} available)",
                item.product_name, item.free_quantity
            )));
        }
    }

    let total_amount_gbp: Decimal = cart_items
        .iter()
        .map(|i| i.quantity * i.price_per_unit)
        .sum();

    let stock_id = cart_items[0].stock_id;
    // ── Transaction ────────────────────────────────────────────────────────
    let mut tx = state.db.begin().await.map_err(AppError::Database)?;

    let order = sqlx::query!(
        r#"
        INSERT INTO orders (
            stock_id, order_key,
            guest_email, guest_phone,
            delivery_address, delivery_county, delivery_postcode,
            total_amount_gbp
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, order_key
        "#,
        stock_id,
        input.order_key,
        input.guest_email,
        input.guest_phone,
        input.delivery_address,
        input.delivery_county,
        input.delivery_postcode,
        total_amount_gbp as Decimal,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    for item in &cart_items {
        let subtotal = item.quantity * item.price_per_unit;
        sqlx::query!(
            r#"
            INSERT INTO order_items
                (order_id, product_id, quantity, unit_price_gbp, subtotal_gbp)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            order.id,
            item.product_id,
            item.quantity       as Decimal,
            item.price_per_unit as Decimal,
            subtotal            as Decimal,
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            r#"
            UPDATE stock
            SET
                quantity_reserved = quantity_reserved + $1,
                stock_status = CASE
                    WHEN (quantity_available - quantity_reserved - $1) <= 0
                        THEN 'out_of_stock'::stock_status
                    WHEN (quantity_available - quantity_reserved - $1) <= low_stock_threshold
                        THEN 'low_stock'::stock_status
                    ELSE 'in_stock'::stock_status
                END,
                updated_at = now()
            WHERE product_id = $2
            "#,
            item.quantity as Decimal,
            item.product_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;
    }

    tx.commit().await.map_err(AppError::Database)?;

    // ── Look up farmer ─────────────────────────────────────────────────────
    let farmer = sqlx::query!(
        r#"
        SELECT p.farmer_id, f.email AS farmer_email
        FROM stock s
        JOIN products p ON p.id = s.product_id
        JOIN farmers  f ON f.id = p.farmer_id
        WHERE s.id = $1
        "#,
        stock_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    // ── Notification row ───────────────────────────────────────────────────
    sqlx::query!(
        r#"
        INSERT INTO notifications (
            farmer_id, order_id, channel,
            recipient_email, event_type, payload, is_sent
        )
        VALUES ($1, $2, 'websocket', $3, 'new_order', $4, false)
        "#,
        farmer.farmer_id,
        order.id,
        farmer.farmer_email,
        serde_json::json!({
            "order_id":         order.id,
            "order_key":        order.order_key,
            "total_amount_gbp": total_amount_gbp,
            "guest_email":      input.guest_email,
        }),
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    // ── WebSocket broadcast ────────────────────────────────────────────────
    let _ = state.broadcaster.publish(WsEvent::for_farmer(
        "order_placed",
        &farmer.farmer_id.to_string(),
        serde_json::json!({
            "order_id":         order.id,
            "order_key":        order.order_key,
            "total_amount_gbp": total_amount_gbp,
            "guest_email":      input.guest_email,
        }),
    ));

    // ── Clear cart ─────────────────────────────────────────────────────────
    sqlx::query!("DELETE FROM cart WHERE id = $1", cart.id)
        .execute(&state.db)
        .await
        .map_err(AppError::Database)?;

    // ── Emails — best-effort, fired after response ─────────────────────────
    let product_name = sqlx::query_scalar!(
        "SELECT name FROM products WHERE id = (SELECT product_id FROM stock WHERE id = $1)",
        stock_id,
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or_else(|_| "Your product".to_string());

    let mailer1        = state.mailer.clone();
    let customer_email = input.guest_email.clone();
    let ok1            = order.order_key.clone();
    let product1       = product_name.clone();
    let total1         = total_amount_gbp.to_string();

    tokio::spawn(async move {
        let _ = mailer1.send(
            &customer_email,
            EmailTemplate::OrderPlaced {
                order_key:        ok1,
                guest_email:      customer_email.clone(),
                total_amount_gbp: total1,
                product_name:     product1,
            },
        ).await;
    });

    let mailer2       = state.mailer.clone();
    let farmer_email  = farmer.farmer_email.clone();
    let ok2           = order.order_key.clone();
    let product2      = product_name.clone();
    let total2        = total_amount_gbp.to_string();
    let customer2     = input.guest_email.clone();

    tokio::spawn(async move {
        let _ = mailer2.send(
            &farmer_email,
            EmailTemplate::FarmerNewOrder {
                order_key:        ok2,
                customer_email:   customer2,
                product_name:     product2,
                total_amount_gbp: total2,
            },
        ).await;
    });

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id":               order.id,
            "order_key":        order.order_key,
            "total_amount_gbp": total_amount_gbp,
            "message":          "Order placed — complete payment to confirm",
        })),
    ))
}
// ── GET /orders/:order_id ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GetOrderQuery {
    pub order_key: Option<String>,
}

async fn get_order(
    State(state):   State<Arc<AppState>>,
    Path(order_id): Path<Uuid>,
    Query(params):  Query<GetOrderQuery>,
) -> AppResult<Json<OrderResponse>> {
    let order = sqlx::query!(
        r#"
        SELECT
            o.id,
            o.order_key,
            o.order_status::text   AS "order_status!: String",
            o.payment_status::text AS "payment_status!: String",
            o.guest_email,
            o.guest_phone,
            o.delivery_address,
            o.delivery_county,
            o.delivery_postcode,
            o.total_amount_gbp,
            o.placed_at
        FROM orders o
        WHERE o.id = $1
        "#,
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

    let items = sqlx::query!(
        r#"
        SELECT
            oi.product_id,
            p.name          AS product_name,
            p.slug          AS product_slug,
            oi.quantity,
            oi.unit_price_gbp,
            oi.subtotal_gbp
        FROM order_items oi
        JOIN products p ON p.id = oi.product_id
        WHERE oi.order_id = $1
        "#,
        order_id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| OrderItemResponse {
        product_id:     r.product_id,
        product_name:   r.product_name,
        product_slug:   r.product_slug,
        quantity:       r.quantity,
        unit_price_gbp: r.unit_price_gbp,
        subtotal_gbp:   r.subtotal_gbp,
    })
    .collect();

    Ok(Json(OrderResponse {
        id:                order.id,
        order_key:         order.order_key,
        order_status:      order.order_status,
        payment_status:    order.payment_status,
        guest_email:       order.guest_email,
        guest_phone:       order.guest_phone,
        delivery_address:  order.delivery_address,
        delivery_county:   order.delivery_county,
        delivery_postcode: order.delivery_postcode,
        total_amount_gbp:  order.total_amount_gbp,
        items,
        placed_at:         order.placed_at,
    }))
}
// ── GET /orders — farmer list ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListOrdersQuery {
    pub page:         Option<i64>,
    pub per_page:     Option<i64>,
    pub order_status: Option<String>,
}

async fn list_orders(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<ListOrdersQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    let page     = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);
    let offset   = (page - 1) * per_page;

    // ── Real total count (unfiltered) ──────────────────────────────────────
    let total: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!: i64"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id = $1
        "#,
        farmer_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    let orders = sqlx::query!(
        r#"
        SELECT
            o.id,
            o.order_key,
            o.guest_email,
            o.order_status::text   AS "order_status!: String",
            o.payment_status::text AS "payment_status!: String",
            o.total_amount_gbp,
            o.placed_at
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id = $1
          AND ($2::text IS NULL OR o.order_status::text = $2)
        ORDER BY o.placed_at DESC
        LIMIT $3 OFFSET $4
        "#,
        farmer_id,
        params.order_status,
        per_page,
        offset,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    let total_pages = ((total as f64) / (per_page as f64)).ceil() as i64;

    Ok(Json(serde_json::json!({
        "data": orders.iter().map(|o| serde_json::json!({
            "id":               o.id,
            "order_key":        o.order_key,
            "guest_email":      o.guest_email,
            "order_status":     o.order_status,
            "payment_status":   o.payment_status,
            "total_amount_gbp": o.total_amount_gbp,
            "placed_at":        o.placed_at,
        })).collect::<Vec<_>>(),
        "total":       total,
        "total_pages": total_pages,
        "page":        page,
        "per_page":    per_page,
    })))
}
// ── PATCH /orders/:order_id/status ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateStatusInput {
    pub order_status: String,
}

async fn update_order_status(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(order_id):              Path<Uuid>,
    Json(input):                 Json<UpdateStatusInput>,
) -> AppResult<StatusCode> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    let allowed = ["confirmed", "processing", "dispatched", "delivered", "cancelled"];
    if !allowed.contains(&input.order_status.as_str()) {
        return Err(AppError::Validation(format!(
            "Invalid status. Must be one of: {}", allowed.join(", ")
        )));
    }

    // ── Fetch order details for email ──────────────────────────────────────
    let order_info = sqlx::query!(
        r#"
        SELECT
            o.guest_email,
            o.order_key,
            p.name AS product_name,
            f.email AS farmer_email
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        JOIN farmers  f ON f.id = p.farmer_id
        WHERE o.id        = $1
          AND p.farmer_id = $2
        "#,
        order_id,
        farmer_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Order not found or not yours".into()))?;

    // ── Update status ──────────────────────────────────────────────────────
    let rows = sqlx::query!(
        r#"
        UPDATE orders o
        SET order_status = $1::order_status,
            updated_at   = now()
        FROM stock s
        JOIN products p ON p.id = s.product_id
        WHERE o.stock_id  = s.id
          AND p.farmer_id = $2
          AND o.id        = $3
        "#,
        &input.order_status as &str,
        farmer_id,
        order_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?
    .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound("Order not found or not yours".into()));
    }

    // ── Release stock on delivered / cancelled ─────────────────────────────
    if input.order_status == "delivered" || input.order_status == "cancelled" {
        let is_delivered = input.order_status == "delivered";
        sqlx::query!(
            r#"
            UPDATE stock s
            SET
                quantity_reserved  = GREATEST(0, s.quantity_reserved - sub.qty),
                quantity_available = CASE
                    WHEN $1 THEN s.quantity_available - sub.qty
                    ELSE s.quantity_available
                END,
                stock_status = CASE
                    WHEN (s.quantity_available
                        - CASE WHEN $1 THEN sub.qty ELSE 0::numeric END
                        - GREATEST(0, s.quantity_reserved - sub.qty)) <= 0
                        THEN 'out_of_stock'::stock_status
                    WHEN (s.quantity_available
                        - CASE WHEN $1 THEN sub.qty ELSE 0::numeric END
                        - GREATEST(0, s.quantity_reserved - sub.qty)) <= s.low_stock_threshold
                        THEN 'low_stock'::stock_status
                    ELSE 'in_stock'::stock_status
                END,
                updated_at = now()
            FROM (
                SELECT SUM(oi.quantity) AS qty, oi.product_id
                FROM order_items oi
                WHERE oi.order_id = $2
                GROUP BY oi.product_id
            ) sub
            WHERE s.product_id = sub.product_id
            "#,
            is_delivered,
            order_id,
        )
        .execute(&state.db)
        .await
        .map_err(AppError::Database)?;
    }

    // ── Email customer — best-effort ───────────────────────────────────────
    let mailer      = state.mailer.clone();
    let to          = order_info.guest_email.clone();
    let ok          = order_info.order_key.clone();
    let product     = order_info.product_name.clone();
    let new_status  = input.order_status.clone();

    tokio::spawn(async move {
        let _ = mailer.send(
            &to,
            EmailTemplate::OrderStatusUpdated {
                order_key:    ok,
                guest_email:  to.clone(),
                new_status,
                product_name: product,
            },
        ).await;
    });

    Ok(StatusCode::NO_CONTENT)
}