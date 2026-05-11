use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use rust_decimal::Decimal;
use chrono::{DateTime, Utc};

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/summary",   get(get_summary))
        .route("/revenue",   get(get_revenue))
        .route("/stock",     get(get_stock))
        .route("/customers", get(get_customers))
        .with_state(state)
}

// ── Query params ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PeriodQuery {
    /// "7d" | "30d" | "90d" | "365d" — default "30d"
    pub period: Option<String>,
}

fn period_days(period: &Option<String>) -> i64 {
    match period.as_deref() {
        Some("7d")   => 7,
        Some("90d")  => 90,
        Some("365d") => 365,
        _            => 30,
    }
}

// ── GET /analytics/summary ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub total_revenue_gbp:   Decimal,
    pub total_orders:        i64,
    pub average_order_value: Decimal,
    pub total_products:      i64,
    pub low_stock_count:     i64,
    pub out_of_stock_count:  i64,
    pub period_days:         i64,
}

async fn get_summary(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<PeriodQuery>,
) -> AppResult<Json<SummaryResponse>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let days = period_days(&params.period);

    let revenue = sqlx::query!(
        r#"
        SELECT
            COALESCE(SUM(o.total_amount_gbp), 0)  AS "total_revenue!: Decimal",
            COUNT(*)                               AS "total_orders!: i64",
            COALESCE(AVG(o.total_amount_gbp), 0)  AS "avg_order!: Decimal"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id    = $1
          AND o.order_status != 'cancelled'
          AND o.placed_at    >= now() - ($2 || ' days')::interval
        "#,
        farmer_id,
        days.to_string(),
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    let products = sqlx::query!(
        r#"
        SELECT
            COUNT(*)                                              AS "total!: i64",
            COUNT(*) FILTER (WHERE s.stock_status = 'low_stock')     AS "low!: i64",
            COUNT(*) FILTER (WHERE s.stock_status = 'out_of_stock')  AS "out!: i64"
        FROM products p
        JOIN stock s ON s.product_id = p.id
        WHERE p.farmer_id  = $1
          AND p.is_active   = true
          AND p.is_deleted  = false
        "#,
        farmer_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok(Json(SummaryResponse {
        total_revenue_gbp:   revenue.total_revenue,
        total_orders:        revenue.total_orders,
        average_order_value: revenue.avg_order,
        total_products:      products.total,
        low_stock_count:     products.low,
        out_of_stock_count:  products.out,
        period_days:         days,
    }))
}

// ── GET /analytics/revenue ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RevenuePoint {
    pub date:        String,
    pub revenue_gbp: Decimal,
    pub orders:      i64,
}

#[derive(Debug, Serialize)]
pub struct TopProduct {
    pub product_name: String,
    pub product_slug: String,
    pub total_revenue: Decimal,
    pub units_sold:    Decimal,
    pub order_count:   i64,
}

#[derive(Debug, Serialize)]
pub struct RevenueResponse {
    pub chart:        Vec<RevenuePoint>,
    pub top_products: Vec<TopProduct>,
}

async fn get_revenue(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<PeriodQuery>,
) -> AppResult<Json<RevenueResponse>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let days = period_days(&params.period);

    let chart = sqlx::query!(
        r#"
        SELECT
            DATE(o.placed_at)::text          AS "date!: String",
            COALESCE(SUM(o.total_amount_gbp), 0) AS "revenue_gbp!: Decimal",
            COUNT(*)                         AS "orders!: i64"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id    = $1
          AND o.order_status != 'cancelled'
          AND o.placed_at    >= now() - ($2 || ' days')::interval
        GROUP BY DATE(o.placed_at)
        ORDER BY DATE(o.placed_at)
        "#,
        farmer_id,
        days.to_string(),
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| RevenuePoint {
        date:        r.date,
        revenue_gbp: r.revenue_gbp,
        orders:      r.orders,
    })
    .collect();

    let top_products = sqlx::query!(
        r#"
        SELECT
            p.name                           AS product_name,
            p.slug                           AS product_slug,
            SUM(oi.subtotal_gbp)             AS "total_revenue!: Decimal",
            SUM(oi.quantity)                 AS "units_sold!: Decimal",
            COUNT(DISTINCT o.id)             AS "order_count!: i64"
        FROM order_items oi
        JOIN orders   o ON o.id = oi.order_id
        JOIN products p ON p.id = oi.product_id
        WHERE p.farmer_id    = $1
          AND o.order_status != 'cancelled'
          AND o.placed_at    >= now() - ($2 || ' days')::interval
        GROUP BY p.id, p.name, p.slug
        ORDER BY SUM(oi.subtotal_gbp) DESC
        LIMIT 5
        "#,
        farmer_id,
        days.to_string(),
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| TopProduct {
        product_name:  r.product_name,
        product_slug:  r.product_slug,
        total_revenue: r.total_revenue,
        units_sold:    r.units_sold,
        order_count:   r.order_count,
    })
    .collect();

    Ok(Json(RevenueResponse { chart, top_products }))
}

// ── GET /analytics/stock ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StockItem {
    pub product_name:        String,
    pub product_slug:        String,
    pub quantity_available:  Decimal,
    pub quantity_reserved:   Decimal,
    pub low_stock_threshold: Decimal,
    pub stock_status:        String,
    pub units_sold_30d:      Decimal,
}

async fn get_stock(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
) -> AppResult<Json<Vec<StockItem>>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let items = sqlx::query!(
        r#"
        SELECT
            p.name                                          AS product_name,
            p.slug                                          AS product_slug,
            s.quantity_available,
            s.quantity_reserved,
            s.low_stock_threshold,
            s.stock_status::text                            AS "stock_status!: String",
            COALESCE(sales.units_sold, 0)                   AS "units_sold_30d!: Decimal"
        FROM products p
        JOIN stock s ON s.product_id = p.id
        LEFT JOIN (
            SELECT
                oi.product_id,
                SUM(oi.quantity) AS units_sold
            FROM order_items oi
            JOIN orders o ON o.id = oi.order_id
            WHERE o.order_status != 'cancelled'
              AND o.placed_at >= now() - interval '30 days'
            GROUP BY oi.product_id
        ) sales ON sales.product_id = p.id
        WHERE p.farmer_id  = $1
          AND p.is_active   = true
          AND p.is_deleted  = false
        ORDER BY s.stock_status, s.quantity_available ASC
        "#,
        farmer_id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| StockItem {
        product_name:        r.product_name,
        product_slug:        r.product_slug,
        quantity_available:  r.quantity_available,
        quantity_reserved:   r.quantity_reserved,
        low_stock_threshold: r.low_stock_threshold,
        stock_status:        r.stock_status,
        units_sold_30d:      r.units_sold_30d,
    })
    .collect();

    Ok(Json(items))
}

// ── GET /analytics/customers ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CustomerMetrics {
    pub unique_customers:     i64,
    pub repeat_customers:     i64,
    pub top_counties:         Vec<CountyCount>,
    pub peak_hours:           Vec<HourCount>,
}

#[derive(Debug, Serialize)]
pub struct CountyCount {
    pub county: String,
    pub orders: i64,
}

#[derive(Debug, Serialize)]
pub struct HourCount {
    pub hour:   i32,
    pub orders: i64,
}

async fn get_customers(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<PeriodQuery>,
) -> AppResult<Json<CustomerMetrics>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let days = period_days(&params.period);

    let unique = sqlx::query!(
        r#"
        SELECT
            COUNT(DISTINCT o.guest_email)                          AS "unique_customers!: i64",
            COUNT(DISTINCT o.guest_email)
                FILTER (WHERE sub.order_count > 1)                 AS "repeat_customers!: i64"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        JOIN (
            SELECT guest_email, COUNT(*) AS order_count
            FROM orders o2
            JOIN stock    s2 ON s2.id = o2.stock_id
            JOIN products p2 ON p2.id = s2.product_id
            WHERE p2.farmer_id    = $1
              AND o2.order_status != 'cancelled'
            GROUP BY guest_email
        ) sub ON sub.guest_email = o.guest_email
        WHERE p.farmer_id    = $1
          AND o.order_status != 'cancelled'
          AND o.placed_at    >= now() - ($2 || ' days')::interval
        "#,
        farmer_id,
        days.to_string(),
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    let top_counties = sqlx::query!(
        r#"
        SELECT
            o.delivery_county   AS county,
            COUNT(*)            AS "orders!: i64"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id    = $1
          AND o.order_status != 'cancelled'
          AND o.placed_at    >= now() - ($2 || ' days')::interval
        GROUP BY o.delivery_county
        ORDER BY COUNT(*) DESC
        LIMIT 5
        "#,
        farmer_id,
        days.to_string(),
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| CountyCount { county: r.county, orders: r.orders })
    .collect();

    let peak_hours = sqlx::query!(
        r#"
        SELECT
            EXTRACT(HOUR FROM o.placed_at)::int  AS "hour!: i32",
            COUNT(*)                             AS "orders!: i64"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id    = $1
          AND o.order_status != 'cancelled'
          AND o.placed_at    >= now() - ($2 || ' days')::interval
        GROUP BY EXTRACT(HOUR FROM o.placed_at)
        ORDER BY COUNT(*) DESC
        LIMIT 6
        "#,
        farmer_id,
        days.to_string(),
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| HourCount { hour: r.hour, orders: r.orders })
    .collect();

    Ok(Json(CustomerMetrics {
        unique_customers:  unique.unique_customers,
        repeat_customers:  unique.repeat_customers,
        top_counties,
        peak_hours,
    }))
}