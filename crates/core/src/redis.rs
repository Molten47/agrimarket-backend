use redis::{aio::ConnectionManager, Client};

/// Creates a Redis ConnectionManager.
/// ConnectionManager auto-reconnects on failure — correct for long-lived services.
pub async fn create_client(redis_url: &str) -> anyhow::Result<ConnectionManager> {
    let client = Client::open(redis_url)?;
    let manager = ConnectionManager::new(client).await?;
    tracing::info!("Redis connection established");
    Ok(manager)
}

/// Key namespacing helpers — centralised so key format never drifts between crates.
pub mod keys {
    /// Blacklisted access tokens (logged-out JTIs)
    pub fn revoked_jti(jti: &str) -> String {
        format!("agrimarket:revoked_jti:{jti}")
    }
    

    /// Cart session
    pub fn cart_session(session_key: &str) -> String {
        format!("agrimarket:cart:{session_key}")
    }

    /// Rate limiting — per IP or per farmer
    pub fn rate_limit(identifier: &str, endpoint: &str) -> String {
        format!("agrimarket:rl:{endpoint}:{identifier}")
    }

    /// WebSocket connection tracking
    pub fn ws_connection(farmer_id: &str) -> String {
        format!("agrimarket:ws:{farmer_id}")
    }
}
