use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/",      get(list_categories).post(create_category))
        .route("/:slug", get(get_category))
        .with_state(state)
}

// ── Response shapes ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CategoryChild {
    pub id:          Uuid,
    pub name:        String,
    pub slug:        String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CategoryResponse {
    pub id:          Uuid,
    pub name:        String,
    pub slug:        String,
    pub description: Option<String>,
    pub parent_id:   Option<Uuid>,
    pub children:    Vec<CategoryChild>,
}

// ── GET /categories ────────────────────────────────────────────────────────────
// Returns top-level categories with their children nested inside.
// Two queries: fetch all active categories, then group in Rust.
// Avoids recursive CTE complexity — tree depth is max 2 by design.

async fn list_categories(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<Vec<CategoryResponse>>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id,
            name,
            slug,
            description,
            parent_id
        FROM categories
        WHERE is_active = true
        ORDER BY parent_id NULLS FIRST, name
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    // Group into tree: parents first, then attach children
    let mut parents: Vec<CategoryResponse> = rows
        .iter()
        .filter(|r| r.parent_id.is_none())
        .map(|r| CategoryResponse {
            id:          r.id,
            name:        r.name.clone(),
            slug:        r.slug.clone(),
            description: r.description.clone(),
            parent_id:   None,
            children:    vec![],
        })
        .collect();

    for row in rows.iter().filter(|r| r.parent_id.is_some()) {
        if let Some(parent) = parents
            .iter_mut()
            .find(|p| Some(p.id) == row.parent_id)
        {
            parent.children.push(CategoryChild {
                id:          row.id,
                name:        row.name.clone(),
                slug:        row.slug.clone(),
                description: row.description.clone(),
            });
        }
    }

    Ok(Json(parents))
}

// ── GET /categories/:slug ──────────────────────────────────────────────────────

async fn get_category(
    State(state): State<Arc<AppState>>,
    Path(slug):   Path<String>,
) -> AppResult<Json<CategoryResponse>> {
    // Fetch the category itself
    let row = sqlx::query!(
        r#"
        SELECT id, name, slug, description, parent_id
        FROM categories
        WHERE slug = $1 AND is_active = true
        "#,
        slug,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Category not found".into()))?;

    // Fetch its children (if it's a parent category)
    let children = sqlx::query!(
        r#"
        SELECT id, name, slug, description
        FROM categories
        WHERE parent_id = $1 AND is_active = true
        ORDER BY name
        "#,
        row.id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|c| CategoryChild {
        id:          c.id,
        name:        c.name,
        slug:        c.slug,
        description: c.description,
    })
    .collect();

    Ok(Json(CategoryResponse {
        id:          row.id,
        name:        row.name,
        slug:        row.slug,
        description: row.description,
        parent_id:   row.parent_id,
        children,
    }))
}

// ── POST /categories ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateCategoryInput {
    name:        String,
    slug:        String,
    description: Option<String>,
    parent_id:   Option<Uuid>,
}

async fn create_category(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Json(input):                 Json<CreateCategoryInput>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    // Validate slug format — lowercase, hyphens only
    if !input.slug.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return Err(AppError::Validation(
            "Slug must be lowercase letters and hyphens only".into(),
        ));
    }

    // If parent_id provided, verify it exists and is itself a top-level category
    // (max tree depth = 2 — no grandchildren)
    if let Some(parent_id) = input.parent_id {
        let parent = sqlx::query!(
            "SELECT parent_id FROM categories WHERE id = $1 AND is_active = true",
            parent_id,
        )
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Parent category not found".into()))?;

        if parent.parent_id.is_some() {
            return Err(AppError::Validation(
                "Cannot nest more than two levels deep".into(),
            ));
        }
    }

    let row = sqlx::query!(
        r#"
        INSERT INTO categories (name, slug, description, parent_id)
        VALUES ($1, $2, $3, $4)
        RETURNING id, slug
        "#,
        input.name,
        input.slug,
        input.description,
        input.parent_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("categories_slug_key") =>
        {
            AppError::Conflict(format!("Slug '{}' is already taken", input.slug))
        }
        other => AppError::Database(other),
    })?;

    let _ = claims; // farmer authenticated — role enforcement added later

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id":   row.id,
            "slug": row.slug,
        })),
    ))
}