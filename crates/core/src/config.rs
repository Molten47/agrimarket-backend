use serde::Deserialize;
use dotenvy::dotenv;

/// All configuration values read from environment variables.
/// Load once at startup via `AppConfig::load()`.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    // ── Server ──────────────────────────────────────────────
    pub host:          String,
    pub port:          u16,
    pub environment:   Environment,

    // ── Database ────────────────────────────────────────────
    pub database_url:  String,
    pub db_max_conns:  u32,

    // ── Redis ────────────────────────────────────────────────
    pub redis_url:     String,

    // ── Auth ─────────────────────────────────────────────────
    /// RS256 private key — PEM format, base64 encoded in env
    pub jwt_private_key_b64: String,
    /// RS256 public key — PEM format, base64 encoded in env
    pub jwt_public_key_b64:  String,
    pub access_token_ttl_secs:  u64,   // default: 900  (15 min)
    pub refresh_token_ttl_secs: u64,   // default: 604800 (7 days)
    

    // ── Stripe ───────────────────────────────────────────────
    pub stripe_secret_key:     String,
    pub stripe_webhook_secret: String,

    // ── Email (Resend) ───────────────────────────────────────
    pub resend_api_key:   String,
    pub email_from:       String,   // e.g. "AgriMarket <noreply@agrimarket.co.uk>"
    pub email_from_name: String,

    // ── Frontend ─────────────────────────────────────────────
    pub frontend_url: String,   // for CORS + email links

    // ── Cloudinary ────────────────────────────────────────────────────────
    pub cloudinary_cloud_name: String,
    pub cloudinary_api_key:    String,
    pub cloudinary_api_secret: String,
   
   
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Development,
    Production,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        // Load .env in dev — silently ignore if not present (production)
        let _ = dotenv();

        let cfg = config::Config::builder()
            .add_source(config::Environment::default().separator("__"))
            // Defaults
            .set_default("host",                    "0.0.0.0")?
            .set_default("port",                    8080i64)?
            .set_default("environment",             "development")?
            .set_default("db_max_conns",            10i64)?
            .set_default("access_token_ttl_secs",   900i64)?
            .set_default("refresh_token_ttl_secs",  604800i64)?
            .build()?;

        Ok(cfg.try_deserialize()?)
    }

    pub fn is_production(&self) -> bool {
        self.environment == Environment::Production
    }
}
