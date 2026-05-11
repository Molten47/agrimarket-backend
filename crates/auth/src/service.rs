use chrono::Utc;
use redis::{aio::ConnectionManager, AsyncCommands};
use sqlx::PgPool;
use uuid::Uuid;
use jsonwebtoken::{EncodingKey, DecodingKey};

use app_core::{config::AppConfig, redis::keys};
use crate::{
    error::AuthError,
    hash::{hash_password, verify_password},
    jwt::{generate_access_token, decoding_key_from_b64, encoding_key_from_b64},
};

// ── Request / Response shapes ─────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, validator::Validate)]
pub struct RegisterInput {
    #[validate(email(message = "Invalid email address"))]
    pub email:     String,
    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password:  String,
    #[validate(length(min = 2, message = "Farm name is required"))]
    pub farm_name: String,
    #[validate(length(min = 2, message = "Full name is required"))]
    pub full_name: String,
    pub phone:     Option<String>,
    #[validate(length(min = 2, message = "County is required"))]
    pub county:    String,
    #[validate(length(min = 5, message = "Valid UK postcode required"))]
    pub postcode:  String,
    pub bio:       Option<String>,
}

#[derive(Debug, serde::Deserialize, validator::Validate)]
pub struct LoginInput {
    #[validate(email)]
    pub email:    String,
    pub password: String,
}

#[derive(Debug, serde::Serialize)]
pub struct AuthTokens {
    pub access_token:  String,
    pub refresh_token: String,
    pub token_type:    String,
    pub expires_in:    u64,
}

#[derive(Debug, serde::Serialize)]
pub struct FarmerResponse {
    pub id:        Uuid,
    pub email:     String,
    pub farm_name: String,
    pub full_name: String,
    pub county:    String,
    pub postcode:  String,
}

// ── Internal DB projections ───────────────────────────────────────────────────

/// Used for register — no password_hash or is_active returned
struct RegisterRow {
    id:        Uuid,
    email:     String,
    farm_name: String,
    full_name: String,
    county:    String,
    postcode:  String,
}

/// Used for login — needs password_hash and is_active for verification
struct LoginRow {
    id:            Uuid,
    email:         String,
    password_hash: String,
    farm_name:     String,
    full_name:     String,
    county:        String,
    postcode:      String,
    is_active:     bool,
    is_verified:   bool,
}

impl From<RegisterRow> for FarmerResponse {
    fn from(f: RegisterRow) -> Self {
        Self {
            id: f.id, email: f.email, farm_name: f.farm_name,
            full_name: f.full_name, county: f.county, postcode: f.postcode,
        }
    }
}

impl From<LoginRow> for FarmerResponse {
    fn from(f: LoginRow) -> Self {
        Self {
            id: f.id, email: f.email, farm_name: f.farm_name,
            full_name: f.full_name, county: f.county, postcode: f.postcode,
        }
    }
}

// ── AuthService ───────────────────────────────────────────────────────────────

pub struct AuthService {
    pub encoding_key: EncodingKey,
    pub decoding_key: DecodingKey,
    pub access_ttl:   u64,
    pub refresh_ttl:  u64,
}

impl AuthService {
    pub fn from_config(config: &AppConfig) -> anyhow::Result<Self> {
        Ok(Self {
            encoding_key: encoding_key_from_b64(&config.jwt_private_key_b64)?,
            decoding_key: decoding_key_from_b64(&config.jwt_public_key_b64)?,
            access_ttl:   config.access_token_ttl_secs,
            refresh_ttl:  config.refresh_token_ttl_secs,
        })
    }

    // ── Register ──────────────────────────────────────────────────────────────

    pub async fn register(
        &self,
        db:    &PgPool,
        input: RegisterInput,
    ) -> Result<(FarmerResponse, AuthTokens), AuthError> {
        // 1. Check email uniqueness
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM farmers WHERE email = $1)"
        )
        .bind(&input.email)
        .fetch_one(db)
        .await?;

        if exists {
            return Err(AuthError::EmailTaken);
        }

        // 2. Hash password in blocking thread — Argon2id is intentionally slow
        let password = input.password.clone();
        let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
            .await
            .map_err(|_| AuthError::HashFailed)??;

        // 3. Insert farmer — return only the fields we need (no hash returned)
        let row = sqlx::query_as!(
            RegisterRow,
            r#"
            INSERT INTO farmers
                (email, password_hash, farm_name, full_name, phone, county, postcode, bio)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, email, farm_name, full_name, county, postcode
            "#,
            input.email,
            password_hash,
            input.farm_name,
            input.full_name,
            input.phone,
            input.county,
            input.postcode,
            input.bio,
        )
        .fetch_one(db)
        .await?;

        // 4. Issue tokens
        let tokens = self.issue_token_pair(db, &row.id, &row.email).await?;

        Ok((row.into(), tokens))
    }

    // ── Login ─────────────────────────────────────────────────────────────────

    pub async fn login(
        &self,
        db:    &PgPool,
        input: LoginInput,
    ) -> Result<(FarmerResponse, AuthTokens), AuthError> {
        // 1. Fetch farmer — same error whether email exists or not (no enumeration)
        let row = sqlx::query_as!(
            LoginRow,
            r#"
            SELECT id, email, password_hash, farm_name, full_name,
                   county, postcode, is_active, is_verified
            FROM farmers
            WHERE email = $1
            "#,
            input.email,
        )
        .fetch_optional(db)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

        // 2. Verify password in blocking thread
        let password = input.password.clone();
        let hash     = row.password_hash.clone();
        tokio::task::spawn_blocking(move || verify_password(&password, &hash))
            .await
            .map_err(|_| AuthError::InvalidCredentials)??;

        // 3. Check account active
        if !row.is_active {
            return Err(AuthError::AccountInactive);
        }
       // if !row.is_verified {
          //  return Err(AuthError::NotVerified);
       // }

        // 4. Issue tokens
        let tokens = self.issue_token_pair(db, &row.id, &row.email).await?;

        Ok((row.into(), tokens))
    }

    // ── Refresh ───────────────────────────────────────────────────────────────

    pub async fn refresh(
        &self,
        db:            &PgPool,
        refresh_token: &str,
    ) -> Result<AuthTokens, AuthError> {
        let token_hash = sha256_hex(refresh_token);

        let record = sqlx::query!(
            r#"
            SELECT id, farmer_id, family_id, is_revoked, expires_at
            FROM refresh_tokens
            WHERE token_hash = $1
            "#,
            token_hash,
        )
        .fetch_optional(db)
        .await?
        .ok_or(AuthError::TokenInvalid)?;

        // Compromise detection — reused token = revoke entire family
        if record.is_revoked {
            sqlx::query!(
                "UPDATE refresh_tokens SET is_revoked = true, revoked_at = now()
                 WHERE family_id = $1",
                record.family_id,
            )
            .execute(db)
            .await?;
            return Err(AuthError::TokenCompromised);
        }

        if record.expires_at < Utc::now() {
            return Err(AuthError::TokenExpired);
        }

        // Revoke the used token
        sqlx::query!(
            "UPDATE refresh_tokens SET is_revoked = true, revoked_at = now()
             WHERE id = $1",
            record.id,
        )
        .execute(db)
        .await?;

        // Fetch farmer and issue new pair
        let farmer = sqlx::query!(
            "SELECT id, email FROM farmers WHERE id = $1",
            record.farmer_id,
        )
        .fetch_one(db)
        .await?;

        self.issue_token_pair(db, &farmer.id, &farmer.email).await
    }

    // ── Logout ────────────────────────────────────────────────────────────────

    pub async fn logout(
        &self,
        db:            &PgPool,
        redis:         &mut ConnectionManager,
        jti:           &str,
        access_exp:    i64,
        refresh_token: Option<&str>,
    ) -> Result<(), AuthError> {
        // Blacklist access token jti in Redis until it expires naturally
        let ttl = (access_exp - Utc::now().timestamp()).max(0) as u64;
        if ttl > 0 {
            let key = keys::revoked_jti(jti);
            redis.set_ex::<_, _, ()>(key, "1", ttl).await?;
        }

        // Revoke refresh token in Postgres if provided
        if let Some(token) = refresh_token {
            let token_hash = sha256_hex(token);
            sqlx::query!(
                "UPDATE refresh_tokens SET is_revoked = true, revoked_at = now()
                 WHERE token_hash = $1",
                token_hash,
            )
            .execute(db)
            .await?;
        }

        Ok(())
    }

    // ── Internal: issue token pair ────────────────────────────────────────────

    async fn issue_token_pair(
        &self,
        db:        &PgPool,
        farmer_id: &Uuid,
        email:     &str,
    ) -> Result<AuthTokens, AuthError> {
        let (access_token, _jti) = generate_access_token(
            farmer_id,
            email,
            self.access_ttl,
            &self.encoding_key,
        )?;

        let raw_refresh = generate_opaque_token();
        let token_hash  = sha256_hex(&raw_refresh);
        let family_id   = Uuid::new_v4();
        let expires_at  = Utc::now() + chrono::Duration::seconds(self.refresh_ttl as i64);

        sqlx::query!(
            r#"
            INSERT INTO refresh_tokens (farmer_id, token_hash, family_id, expires_at)
            VALUES ($1, $2, $3, $4)
            "#,
            farmer_id,
            token_hash,
            family_id,
            expires_at,
        )
        .execute(db)
        .await?;

        Ok(AuthTokens {
            access_token,
            refresh_token: raw_refresh,
            token_type:    "Bearer".into(),
            expires_in:    self.access_ttl,
        })
    }
}

// ── Crypto helpers ────────────────────────────────────────────────────────────

fn generate_opaque_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn sha256_hex(input: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}