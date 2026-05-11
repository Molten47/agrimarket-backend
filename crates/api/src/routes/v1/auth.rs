use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use validator::Validate;

use app_core::error::{AppError, AppResult};
use auth::service::{RegisterInput, LoginInput};
use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/register", post(register))
        .route("/login",    post(login))
        .route("/refresh",  post(refresh))
        .route("/logout",   post(logout))
        .route("/verify",   get(verify_email))
        .with_state(state)
}

// ── Register ──────────────────────────────────────────────────────────────────

async fn register(
    State(state): State<Arc<AppState>>,
    Json(input):  Json<RegisterInput>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    input.validate().map_err(|e| AppError::Validation(e.to_string()))?;

    let (farmer, tokens) = state.auth
        .register(&state.db, input)
        .await
        .map_err(AppError::from)?;

    // ── Generate verification token ────────────────────────────────────────
    let token = generate_token();

 sqlx::query!(
    r#"
    INSERT INTO verification_tokens (farmer_id, token)
    VALUES ($1, $2)
    ON CONFLICT (farmer_id) DO UPDATE SET
        token      = EXCLUDED.token,
        expires_at = now() + interval '24 hours'
    "#,
    farmer.id,
    token,
)
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    // ── Send verification email — best effort ──────────────────────────────
    let mailer     = state.mailer.clone();
    let to         = farmer.email.clone();
    let farm_name  = farmer.farm_name.clone();
    let token_copy = token.clone();

    tokio::spawn(async move {
        let _ = mailer.send(
            &to,
            mailer::templates::EmailTemplate::VerifyEmail {
                farm_name,
                verify_url: format!(
                    "http://localhost:5173/verify-email?token={}",
                    token_copy
                ),
            },
        ).await;
    });

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "farmer": farmer,
            "tokens": tokens,
            "message": "Account created — please check your email to verify your account",
        })),
    ))
}

// ── Login ─────────────────────────────────────────────────────────────────────

async fn login(
    State(state): State<Arc<AppState>>,
    Json(input):  Json<LoginInput>,
) -> AppResult<Json<serde_json::Value>> {
    input.validate().map_err(|e| AppError::Validation(e.to_string()))?;

    let (farmer, tokens) = state.auth
        .login(&state.db, input)
        .await
        .map_err(AppError::from)?;

    Ok(Json(serde_json::json!({
        "farmer": farmer,
        "tokens": tokens,
    })))
}

// ── Refresh ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RefreshInput {
    refresh_token: String,
}

async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(input):  Json<RefreshInput>,
) -> AppResult<Json<serde_json::Value>> {
    let tokens = state.auth
        .refresh(&state.db, &input.refresh_token)
        .await
        .map_err(AppError::from)?;

    Ok(Json(serde_json::json!({ "tokens": tokens })))
}

// ── Logout ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LogoutInput {
    /// The jti from the current access token — extracted by the client from
    /// the decoded JWT payload before sending. Included so we can blacklist it.
    jti:           String,
    /// Unix timestamp of access token expiry — used to set Redis TTL precisely.
    access_exp:    i64,
    /// Optional — if provided, the refresh token is also revoked in Postgres.
    refresh_token: Option<String>,
}

async fn logout(
    State(state): State<Arc<AppState>>,
    Json(input):  Json<LogoutInput>,
) -> AppResult<StatusCode> {
    let mut redis = state.redis.clone();

    state.auth
        .logout(
            &state.db,
            &mut redis,
            &input.jti,
            input.access_exp,
            input.refresh_token.as_deref(),
        )
        .await
        .map_err(AppError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Verify email ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct VerifyQuery {
    token: String,
}

async fn verify_email(
    State(state):  State<Arc<AppState>>,
    Query(params): Query<VerifyQuery>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    let record = sqlx::query!(
        r#"
        SELECT farmer_id, expires_at
        FROM verification_tokens
        WHERE token = $1
        "#,
        params.token,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?;

    // If token already used, check if farmer is verified — if so, treat as success
    if record.is_none() {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "message": "Email verified — you can now log in",
            })),
        ));
    }

    let record = record.unwrap();

    if record.expires_at < chrono::Utc::now() {
        return Err(AppError::Validation(
            "Verification link has expired — please register again".into()
        ));
    }

    sqlx::query!(
        "UPDATE farmers SET is_verified = true WHERE id = $1",
        record.farmer_id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    sqlx::query!(
        "DELETE FROM verification_tokens WHERE token = $1",
        params.token,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Email verified — you can now log in",
        })),
    ))
}

// ── Token generator ────────────────────────────────────────────────────────────

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..64)
        .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
        .collect()
}