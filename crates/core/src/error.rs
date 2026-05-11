use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// The canonical error type for AgriMarket.
/// Every crate converts its internal errors into this.
#[derive(Debug, Error)]
pub enum AppError {
    // ── Auth ─────────────────────────────────────────────────
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Token expired")]
    TokenExpired,

    #[error("Token invalid")]
    TokenInvalid,

    #[error("Refresh token reuse detected — all sessions revoked")]
    TokenCompromised,

    #[error("Unauthorised")]
    Unauthorised,

    #[error("Forbidden")]
    Forbidden,
    
    #[error("Account not verified — please check your email")]
    NotVerified,

    // ── Resource ─────────────────────────────────────────────
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    // ── Validation ───────────────────────────────────────────
    #[error("Validation failed: {0}")]
    Validation(String),

    // ── Payment ──────────────────────────────────────────────
    #[error("Payment failed: {0}")]
    PaymentFailed(String),

    #[error("Webhook signature invalid")]
    WebhookSignatureInvalid,

    // ── Database ─────────────────────────────────────────────
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    // ── Redis ────────────────────────────────────────────────
    #[error("Cache error: {0}")]
    Cache(#[from] redis::RedisError),

    // ── Internal ─────────────────────────────────────────────
    #[error("Internal server error")]
    Internal(#[from] anyhow::Error),
}

/// Maps AppError to HTTP response with the shared error envelope:
/// { "error": { "code": "...", "message": "..." } }
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::InvalidCredentials       => (StatusCode::UNAUTHORIZED,            "INVALID_CREDENTIALS"),
            AppError::TokenExpired             => (StatusCode::UNAUTHORIZED,            "TOKEN_EXPIRED"),
            AppError::TokenInvalid             => (StatusCode::UNAUTHORIZED,            "TOKEN_INVALID"),
            AppError::TokenCompromised         => (StatusCode::UNAUTHORIZED,            "TOKEN_COMPROMISED"),
            AppError::Unauthorised             => (StatusCode::UNAUTHORIZED,            "UNAUTHORISED"),
            AppError::Forbidden                => (StatusCode::FORBIDDEN,               "FORBIDDEN"),
            AppError::NotVerified              => (StatusCode::FORBIDDEN, "NOT_VERIFIED"),
            AppError::NotFound(_)              => (StatusCode::NOT_FOUND,               "NOT_FOUND"),
            AppError::Conflict(_)              => (StatusCode::CONFLICT,                "CONFLICT"),
            AppError::Validation(_)            => (StatusCode::UNPROCESSABLE_ENTITY,    "VALIDATION_ERROR"),
            AppError::PaymentFailed(_)         => (StatusCode::PAYMENT_REQUIRED,        "PAYMENT_FAILED"),
            AppError::WebhookSignatureInvalid  => (StatusCode::UNAUTHORIZED,            "WEBHOOK_INVALID"),
            AppError::Database(_)              => (StatusCode::INTERNAL_SERVER_ERROR,   "DB_ERROR"),
            AppError::Cache(_)                 => (StatusCode::INTERNAL_SERVER_ERROR,   "CACHE_ERROR"),
            AppError::Internal(_)              => (StatusCode::INTERNAL_SERVER_ERROR,   "INTERNAL_ERROR"),
        };

        // Don't leak internal error details in production
        let message = match &self {
            AppError::Database(_) | AppError::Cache(_) | AppError::Internal(_) => {
                tracing::error!(error = %self, "Internal error");
                "An unexpected error occurred".to_string()
            }
            other => other.to_string(),
        };

        let body = json!({
            "error": {
                "code":    code,
                "message": message,
            }
        });

        (status, Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
