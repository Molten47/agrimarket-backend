use base64::Engine;
use chrono::Utc;
use jsonwebtoken::{
    decode, encode, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::error::AuthError;

// ── Claims ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub:   String,
    pub jti:   String,
    pub email: String,
    pub iat:   i64,
    pub exp:   i64,
}

// ── Key loading ───────────────────────────────────────────────────────────────

pub fn encoding_key_from_b64(b64: &str) -> anyhow::Result<EncodingKey> {
    let pem = base64::engine::general_purpose::STANDARD.decode(b64)?;
    Ok(EncodingKey::from_rsa_pem(&pem)?)
}

pub fn decoding_key_from_b64(b64: &str) -> anyhow::Result<DecodingKey> {
    let pem = base64::engine::general_purpose::STANDARD.decode(b64)?;
    // from_rsa_pem accepts the public key PEM directly
    Ok(DecodingKey::from_rsa_pem(&pem)?)
}

// ── Token generation ──────────────────────────────────────────────────────────

pub fn generate_access_token(
    farmer_id:    &Uuid,
    email:        &str,
    ttl_secs:     u64,
    encoding_key: &EncodingKey,
) -> Result<(String, String), AuthError> {
    let now = Utc::now().timestamp();
    let jti = Uuid::new_v4().to_string();

    let claims = Claims {
        sub:   farmer_id.to_string(),
        jti:   jti.clone(),
        email: email.to_string(),
        iat:   now,
        exp:   now + ttl_secs as i64,
    };

    let token = encode(&Header::new(Algorithm::RS256), &claims, encoding_key)
        .map_err(|_| AuthError::TokenInvalid)?;

    Ok((token, jti))
}

// ── Token verification ────────────────────────────────────────────────────────

pub fn verify_access_token(
    token:        &str,
    decoding_key: &DecodingKey,
) -> Result<TokenData<Claims>, AuthError> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = true;

    decode::<Claims>(token, decoding_key, &validation).map_err(|e| {
        use jsonwebtoken::errors::ErrorKind;
        match e.kind() {
            ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            _ => AuthError::TokenInvalid,
        }
    })
}