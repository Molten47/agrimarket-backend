use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Json,
};
use axum::{routing::post, Router};
use axum::extract::DefaultBodyLimit;
use std::sync::Arc;
use crate::state::AppState;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use app_core::error::{AppError, AppResult};
use crate::{middleware::AuthenticatedFarmer};


pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/image", post(upload_image))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .with_state(state)
}

pub async fn upload_image(
    State(state):                State<Arc<AppState>>,
    AuthenticatedFarmer(_claims): AuthenticatedFarmer,
    mut multipart:               Multipart,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    // Extract file bytes from multipart
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name = "upload".to_string();
    tracing::info!("Upload handler reached");
    

  while let Some(field) = multipart
    .next_field()
    .await
    .map_err(|e| {
        tracing::error!("Multipart field error: {:?}", e);
        AppError::Validation(e.to_string())
    })?

    {
        if field.name() == Some("image") {
            file_name = field
                .file_name()
                .unwrap_or("upload")
                .to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(e.to_string()))?;
            file_bytes = Some(bytes.to_vec());
        }
    }

    let bytes = file_bytes
        .ok_or_else(|| AppError::Validation("No image field in form".into()))?;

    // Validate file size — 5MB max
    if bytes.len() > 5 * 1024 * 1024 {
        return Err(AppError::Validation("Image must be under 5MB".into()));
    }

    // Build Cloudinary signed upload
    let cloud_name = &state.config.cloudinary_cloud_name;
    let api_key    = &state.config.cloudinary_api_key;
    let api_secret = &state.config.cloudinary_api_secret;

    let timestamp = chrono::Utc::now().timestamp().to_string();
    let folder    = "agrimarket/products";

    // Signature: SHA256("folder=agrimarket/products&timestamp=<ts><secret>")
    let sign_str = format!(
        "folder={}&timestamp={}{}",
        folder, timestamp, api_secret
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(api_secret.as_bytes())
        .expect("HMAC init failed");
    mac.update(sign_str.as_bytes());

    // Cloudinary uses raw SHA1 for upload signatures — use sha1 crate
    // Actually Cloudinary expects SHA-1 not HMAC — compute manually:
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(sign_str.as_bytes());
    let signature = hex::encode(hasher.finalize());

    // Build multipart form for Cloudinary
    let file_part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str("image/jpeg")
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let form = reqwest::multipart::Form::new()
        .text("api_key",   api_key.clone())
        .text("timestamp", timestamp)
        .text("folder",    folder)
        .text("signature", signature)
        .part("file",      file_part);

    let url = format!(
        "https://api.cloudinary.com/v1_1/{}/image/upload",
        cloud_name
    );

    let client   = reqwest::Client::new();
    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| AppError::Validation(format!("Cloudinary request failed: {e}")))?;

    if !response.status().is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(AppError::Validation(format!("Cloudinary error: {err}")));
    }

    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let image_url = result["secure_url"]
        .as_str()
        .ok_or_else(|| AppError::Validation("No secure_url in Cloudinary response".into()))?
        .to_string();

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "image_url": image_url })),
    ))
}