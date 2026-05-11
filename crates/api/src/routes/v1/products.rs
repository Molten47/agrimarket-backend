use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, patch, post},
    Json, Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/",      get(list_products).post(create_product))
        .route("/:slug", get(get_product).patch(update_product).delete(delete_product))
        .with_state(state)
}

// ── Response shape ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct ProductResponse {
    pub id:                  Uuid,
    pub slug:                String,
    pub name:                String,
    pub description:         Option<String>,
    pub price_per_unit:      Decimal,
    pub unit:                String,
    pub category_slug:       Option<String>,
    pub category_name:       Option<String>,
    pub farmer_id:           Uuid,
    pub farm_name:           String,
    pub county:              String,
    pub stock_status:        String,
    pub quantity_available:  Decimal,
    pub created_at:          DateTime<Utc>,
    pub image_url: Option<String>,
}

// ── Query params ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListProductsQuery {
    pub page:          Option<i64>,
    pub per_page:      Option<i64>,
    pub category_slug: Option<String>,
    pub county:        Option<String>,
    pub stock_status:  Option<String>,
    pub search:        Option<String>,
}

// ── GET /products ──────────────────────────────────────────────────────────────

async fn list_products(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListProductsQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let page     = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);
    let offset   = (page - 1) * per_page;

    // category_slug filter: match the category itself OR any child of it
    // categories uses parent_id (UUID FK), so we resolve slug → id first
    let products = sqlx::query_as!(
        ProductResponse,
        r#"
        SELECT
            p.id,
            p.slug,
            p.name,
            p.description,
            p.price_per_unit,
            p.unit,
            c.slug          AS category_slug,
            c.name          AS category_name,
            f.id            AS farmer_id,
            f.farm_name,
            f.county,
            s.stock_status::text AS "stock_status!: String",
            s.quantity_available,
            p.created_at,
            p.image_url
        FROM products p
        JOIN farmers    f ON f.id = p.farmer_id
        JOIN stock      s ON s.product_id = p.id
        JOIN categories c ON c.id = p.category_id
        WHERE p.is_active = true
          AND p.is_deleted = false
          AND f.is_active = true
          AND (
              $1::text IS NULL
              OR c.slug = $1
              OR c.parent_id = (SELECT id FROM categories WHERE slug = $1)
          )
          AND ($2::text IS NULL OR f.county ILIKE $2)
          AND ($3::text IS NULL OR s.stock_status::text = $3)
          AND (
              $4::text IS NULL
              OR p.name ILIKE '%' || $4 || '%'
              OR p.description ILIKE '%' || $4 || '%'
          )
        ORDER BY p.created_at DESC
        LIMIT $5 OFFSET $6
        "#,
        params.category_slug,
        params.county,
        params.stock_status,
        params.search,
        per_page,
        offset,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    let total: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM products p
        JOIN farmers    f ON f.id = p.farmer_id
        JOIN stock      s ON s.product_id = p.id
        JOIN categories c ON c.id = p.category_id
        WHERE p.is_active = true
          AND p.is_deleted = false
          AND f.is_active = true
          AND (
              $1::text IS NULL
              OR c.slug = $1
              OR c.parent_id = (SELECT id FROM categories WHERE slug = $1)
          )
          AND ($2::text IS NULL OR f.county ILIKE $2)
          AND ($3::text IS NULL OR s.stock_status::text = $3)
          AND (
              $4::text IS NULL
              OR p.name ILIKE '%' || $4 || '%'
              OR p.description ILIKE '%' || $4 || '%'
          )
        "#,
        params.category_slug,
        params.county,
        params.stock_status,
        params.search,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "data":        products,
        "page":        page,
        "per_page":    per_page,
        "total":       total,
        "total_pages": (total as f64 / per_page as f64).ceil() as i64,
    })))
}

// ── GET /products/:slug ────────────────────────────────────────────────────────

async fn get_product(
    State(state): State<Arc<AppState>>,
    Path(slug):   Path<String>,
) -> AppResult<Json<ProductResponse>> {
    let product = sqlx::query_as!(
        ProductResponse,
        r#"
        SELECT
            p.id,
            p.slug,
            p.name,
            p.description,
            p.price_per_unit,
            p.unit,
            c.slug          AS category_slug,
            c.name          AS category_name,
            f.id            AS farmer_id,
            f.farm_name,
            f.county,
            s.stock_status::text AS "stock_status!: String",
            s.quantity_available,
            p.created_at,
            p.image_url
        FROM products p
        JOIN farmers    f ON f.id = p.farmer_id
        JOIN stock      s ON s.product_id = p.id
        JOIN categories c ON c.id = p.category_id
        WHERE p.slug      = $1
          AND p.is_active  = true
          AND p.is_deleted = false
        "#,
        slug,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Product not found".into()))?;

    Ok(Json(product))
}

// ── POST /products ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateProductInput {
    name:                String,
    description:         Option<String>,
    price_per_unit:      Decimal,
    unit:                String,
    category_id:         Uuid,          // required — category_id NOT NULL in DB
    quantity_available:  Decimal,
    low_stock_threshold: Option<Decimal>,
    image_url: Option<String>,
    
}

async fn create_product(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Json(input):                 Json<CreateProductInput>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    // Slug: lowercase name, spaces → hyphens, append short UUID for uniqueness
    let slug = format!(
        "{}-{}",
        input.name.to_lowercase().replace(' ', "-"),
        &Uuid::new_v4().to_string()[..8]
    );

    // Compute initial stock_status from quantity vs threshold
    let threshold = input.low_stock_threshold
        .unwrap_or(Decimal::from(5));

    let stock_status = if input.quantity_available == Decimal::ZERO {
        "out_of_stock"
    } else if input.quantity_available <= threshold {
        "low_stock"
    } else {
        "in_stock"
    };

    // Transaction: product + stock inserted atomically (SDA — no product without stock)
    let mut tx = state.db.begin().await.map_err(AppError::Database)?;

    let product = sqlx::query!(
        r#"
            INSERT INTO products
            (farmer_id, category_id, name, slug, description, price_per_unit, unit, image_url)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, slug
        "#,
        farmer_id,
        input.category_id,
        input.name,
        slug,
        input.description,
        input.price_per_unit,
        input.unit,
        input.image_url
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

sqlx::query!(
    r#"
    INSERT INTO stock
        (product_id, quantity_available, low_stock_threshold, stock_status)
    VALUES ($1, $2, $3, $4::stock_status)
    "#,
    product.id,
    input.quantity_available as Decimal,
    threshold as Decimal,
    stock_status as &str,
)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id":      product.id,
            "slug":    product.slug,
            "message": "Product created successfully",
        })),
    ))
}

// ── PATCH /products/:slug ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct UpdateProductInput {
    name:           Option<String>,
    description:    Option<String>,
    price_per_unit: Option<Decimal>,
    unit:           Option<String>,
    category_id:    Option<Uuid>,
}

async fn update_product(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(slug):                  Path<String>,
    Json(input):                 Json<UpdateProductInput>,
) -> AppResult<StatusCode> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    // COALESCE: only fields sent in the body are updated
    // AND farmer_id = $8 enforces ownership — farmers can't edit each other's products
    let rows_affected = sqlx::query!(
        r#"
        UPDATE products
        SET
            name           = COALESCE($1, name),
            description    = COALESCE($2, description),
            price_per_unit = COALESCE($3, price_per_unit),
            unit           = COALESCE($4, unit),
            category_id    = COALESCE($5, category_id),
            updated_at     = now()
        WHERE slug      = $6
          AND farmer_id = $7
          AND is_active  = true
          AND is_deleted = false
        "#,
        input.name,
        input.description,
        input.price_per_unit,
        input.unit,
        input.category_id,
        slug,
        farmer_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound("Product not found or not yours".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /products/:slug ─────────────────────────────────────────────────────

async fn delete_product(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Path(slug):                  Path<String>,
) -> AppResult<StatusCode> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID in token".into()))?;

    // Soft delete only — is_deleted = true, is_active = false
    // Data is never hard-deleted; audit trail stays intact
    let rows_affected = sqlx::query!(
        r#"
        UPDATE products
        SET is_active  = false,
            is_deleted = true,
            updated_at = now()
        WHERE slug      = $1
          AND farmer_id = $2
          AND is_active  = true
          AND is_deleted = false
        "#,
        slug,
        farmer_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound("Product not found or not yours".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}