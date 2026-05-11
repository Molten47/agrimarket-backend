use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode, HeaderMap},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use redis::AsyncCommands;

use app_core::{error::AppError, redis::keys};
use auth::jwt::{verify_access_token, Claims};
use crate::state::AppState;

// ── Extractor ─────────────────────────────────────────────────────────────────

/// Inject this into any handler that requires an authenticated farmer:
///
/// ```rust
/// async fn my_handler(
///     AuthenticatedFarmer(claims): AuthenticatedFarmer,
///     State(state): State<Arc<AppState>>,
/// ) -> AppResult<Json<Value>> { ... }
/// ```
pub struct AuthenticatedFarmer(pub Claims);

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AuthenticatedFarmer {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 1. Extract Bearer token from Authorization header
        let token = extract_bearer_token(&parts.headers)
            .ok_or_else(|| AppError::Unauthorised.into_response())?;

        // 2. Verify RS256 signature + expiry
        let token_data = verify_access_token(token, &state.auth.decoding_key)
            .map_err(|e| AppError::from(e).into_response())?;

        let claims = token_data.claims;

        // 3. Check jti is not blacklisted in Redis (set on logout)
        let mut redis = state.redis.clone();
        let blacklist_key = keys::revoked_jti(&claims.jti);

        let is_revoked: bool = redis
            .exists(&blacklist_key)
            .await
            .unwrap_or(false);

        if is_revoked {
            return Err(AppError::TokenInvalid.into_response());
        }

        Ok(AuthenticatedFarmer(claims))
    }
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("Authorization")?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}