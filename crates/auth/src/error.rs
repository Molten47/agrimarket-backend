use app_core::error::AppError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Email already registered")]
    EmailTaken,

    #[error("Invalid email or password")]
    InvalidCredentials,

    #[error("Token has expired")]
    TokenExpired,

    #[error("Token is invalid")]
    TokenInvalid,

    #[error("Account not verified")]
    NotVerified,

    #[error("Refresh token has already been used — all sessions revoked")]
    TokenCompromised,

    #[error("Account is deactivated")]
    AccountInactive,

    #[error("Password hashing failed")]
    HashFailed,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Cache error: {0}")]
    Cache(#[from] redis::RedisError),
    
   
}

/// Convert AuthError → AppError so Axum can turn it into an HTTP response
/// without the api crate needing to know about AuthError internals.
impl From<AuthError> for AppError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::EmailTaken          => AppError::Conflict("Email already registered".into()),
            AuthError::InvalidCredentials  => AppError::InvalidCredentials,
            AuthError::TokenExpired        => AppError::TokenExpired,
            AuthError::NotVerified => AppError::NotVerified,
            AuthError::TokenInvalid        => AppError::TokenInvalid,
            AuthError::TokenCompromised    => AppError::TokenCompromised,
            AuthError::AccountInactive     => AppError::Forbidden,
            AuthError::HashFailed          => AppError::Internal(anyhow::anyhow!("Password hashing failed")),
            AuthError::Database(e)         => AppError::Database(e),
            AuthError::Cache(e)            => AppError::Cache(e),

        }
    }
}