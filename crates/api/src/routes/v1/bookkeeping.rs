use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use rust_decimal::Decimal;
use chrono::NaiveDate;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer, state::AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/income",   get(get_income))
        .route("/expenses",  get(list_expenses).post(create_expense))
        .route("/summary",   get(get_summary))
        .with_state(state)
}
// ── Query params ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DateRangeQuery {
    pub from: Option<NaiveDate>,
    pub to:   Option<NaiveDate>,
    pub page: Option<i64>,
}

fn date_from(q: &DateRangeQuery) -> NaiveDate {
    q.from.unwrap_or_else(|| {
        chrono::Utc::now().date_naive() - chrono::Duration::days(30)
    })
}

fn date_to(q: &DateRangeQuery) -> NaiveDate {
    q.to.unwrap_or_else(|| chrono::Utc::now().date_naive())
}

// ── GET /bookkeeping/income ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct IncomeRecord {
    pub order_key:        String,
    pub placed_at:        String,
    pub guest_email:      String,
    pub total_amount_gbp: Decimal,
    pub order_status:     String,
}

async fn get_income(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<DateRangeQuery>,
) -> AppResult<Json<Vec<IncomeRecord>>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let from = date_from(&params);
    let to   = date_to(&params);

    let records = sqlx::query!(
        r#"
        SELECT
            o.order_key,
            o.placed_at::text          AS "placed_at!: String",
            o.guest_email,
            o.total_amount_gbp,
            o.order_status::text       AS "order_status!: String"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id     = $1
          AND o.order_status != 'cancelled'
          AND DATE(o.placed_at) BETWEEN $2 AND $3
        ORDER BY o.placed_at DESC
        LIMIT 100
        "#,
        farmer_id, from, to,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| IncomeRecord {
        order_key:        r.order_key,
        placed_at:        r.placed_at,
        guest_email:      r.guest_email,
        total_amount_gbp: r.total_amount_gbp,
        order_status:     r.order_status,
    })
    .collect();

    Ok(Json(records))
}
// ── Expenses ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ExpenseRecord {
    pub id:           Uuid,
    pub amount_gbp:   Decimal,
    pub category:     String,
    pub description:  Option<String>,
    pub expense_date: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateExpenseInput {
    pub amount_gbp:   Decimal,
    pub category:     String,
    pub description:  Option<String>,
    pub expense_date: NaiveDate,
}

async fn list_expenses(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<DateRangeQuery>,
) -> AppResult<Json<Vec<ExpenseRecord>>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let from = date_from(&params);
    let to   = date_to(&params);

    let records = sqlx::query!(
        r#"
        SELECT id, amount_gbp, category, description,
               expense_date::text AS "expense_date!: String"
        FROM expenses
        WHERE farmer_id    = $1
          AND expense_date BETWEEN $2 AND $3
        ORDER BY expense_date DESC
        "#,
        farmer_id, from, to,
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?
    .into_iter()
    .map(|r| ExpenseRecord {
        id:           r.id,
        amount_gbp:   r.amount_gbp,
        category:     r.category,
        description:  r.description,
        expense_date: r.expense_date,
    })
    .collect();

    Ok(Json(records))
}

async fn create_expense(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Json(input):                 Json<CreateExpenseInput>,
) -> AppResult<(StatusCode, Json<ExpenseRecord>)> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    if input.amount_gbp <= Decimal::ZERO {
        return Err(AppError::Validation("Amount must be positive".into()));
    }
    if input.category.trim().is_empty() {
        return Err(AppError::Validation("Category is required".into()));
    }

    let rec = sqlx::query!(
        r#"
        INSERT INTO expenses (farmer_id, amount_gbp, category, description, expense_date)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, amount_gbp, category, description,
                  expense_date::text AS "expense_date!: String"
        "#,
        farmer_id,
        input.amount_gbp,
        input.category.trim(),
        input.description.as_deref(),
        input.expense_date,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((StatusCode::CREATED, Json(ExpenseRecord {
        id:           rec.id,
        amount_gbp:   rec.amount_gbp,
        category:     rec.category,
        description:  rec.description,
        expense_date: rec.expense_date,
    })))
}
// ── GET /bookkeeping/summary ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BookkeepingSummary {
    pub total_income_gbp:   Decimal,
    pub total_expenses_gbp: Decimal,
    pub net_profit_gbp:     Decimal,
    pub order_count:        i64,
    pub expense_count:      i64,
    pub from:               String,
    pub to:                 String,
}

async fn get_summary(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(claims): AuthenticatedFarmer,
    Query(params):               Query<DateRangeQuery>,
) -> AppResult<Json<BookkeepingSummary>> {
    let farmer_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid farmer ID".into()))?;

    let from = date_from(&params);
    let to   = date_to(&params);

    let income = sqlx::query!(
        r#"
        SELECT
            COALESCE(SUM(o.total_amount_gbp), 0) AS "total!: Decimal",
            COUNT(*)                             AS "count!: i64"
        FROM orders o
        JOIN stock    s ON s.id = o.stock_id
        JOIN products p ON p.id = s.product_id
        WHERE p.farmer_id     = $1
          AND o.order_status != 'cancelled'
          AND DATE(o.placed_at) BETWEEN $2 AND $3
        "#,
        farmer_id, from, to,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    let expenses = sqlx::query!(
        r#"
        SELECT
            COALESCE(SUM(amount_gbp), 0) AS "total!: Decimal",
            COUNT(*)                     AS "count!: i64"
        FROM expenses
        WHERE farmer_id    = $1
          AND expense_date BETWEEN $2 AND $3
        "#,
        farmer_id, from, to,
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    let total_income   = income.total;
    let total_expenses = expenses.total;

    Ok(Json(BookkeepingSummary {
        total_income_gbp:   total_income,
        total_expenses_gbp: total_expenses,
        net_profit_gbp:     total_income - total_expenses,
        order_count:        income.count,
        expense_count:      expenses.count,
        from:               from.to_string(),
        to:                 to.to_string(),
    }))
}
