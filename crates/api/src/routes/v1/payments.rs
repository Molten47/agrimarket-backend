use axum::{
    extract::{State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use rust_decimal::prelude::ToPrimitive;
use mailer::templates::EmailTemplate;

use app_core::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/intent",  post(create_intent))
        .route("/webhook", post(webhook))
        .with_state(state)
}

// ── POST /payments/intent ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateIntentInput {
    pub order_id:  Uuid,
    pub order_key: String,
}

#[derive(Debug, Serialize)]
pub struct CreateIntentResponse {
    pub client_secret:     String,
    pub payment_intent_id: String,
    pub amount_pence:      i64,
}

async fn create_intent(
    State(state): State<Arc<AppState>>,
    Json(input):  Json<CreateIntentInput>,
) -> AppResult<Json<CreateIntentResponse>> {
    let order = sqlx::query!(
        r#"
        SELECT id, order_key, total_amount_gbp, payment_reference
        FROM orders
        WHERE id = $1 AND order_key = $2
        "#,
        input.order_id,
        input.order_key,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound("Order not found".into()))?;

    if let Some(ref existing_ref) = order.payment_reference {
        let intent = stripe_get_intent(&state.config.stripe_secret_key, existing_ref).await?;
        return Ok(Json(CreateIntentResponse {
            client_secret:     intent.client_secret,
            payment_intent_id: intent.id,
            amount_pence:      intent.amount,
        }));
    }

    let amount_pence = (order.total_amount_gbp * rust_decimal::Decimal::from(100))
        .round()
        .to_i64()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Amount conversion failed")))?;

    let intent = stripe_create_intent(
        &state.config.stripe_secret_key,
        amount_pence,
        &order.order_key,
        input.order_id,
    )
    .await?;

    sqlx::query!(
        r#"
        UPDATE orders
        SET payment_reference = $1,
            payment_method    = 'stripe',
            updated_at        = now()
        WHERE id = $2
        "#,
        intent.id,
        order.id,
    )
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok(Json(CreateIntentResponse {
        client_secret:     intent.client_secret,
        payment_intent_id: intent.id,
        amount_pence,
    }))
}

// ── POST /payments/webhook ──────────────────────────────────────────────────

pub async fn webhook(
    State(state): State<Arc<AppState>>,
    headers:      HeaderMap,
    body:         axum::body::Bytes,
) -> AppResult<StatusCode> {
    let sig_header = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::WebhookSignatureInvalid)?;

    verify_stripe_signature(&body, sig_header, &state.config.stripe_webhook_secret)?;

    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| AppError::WebhookSignatureInvalid)?;

    let event_type = payload["type"]
        .as_str()
        .ok_or(AppError::WebhookSignatureInvalid)?;

    let intent_id = payload["data"]["object"]["id"].as_str().unwrap_or("");

    let order_id_str = payload["data"]["object"]["metadata"]["order_id"]
        .as_str()
        .unwrap_or("");

    match event_type {
        "payment_intent.succeeded" => {
            if let Ok(order_id) = Uuid::parse_str(order_id_str) {
                // Mark order paid + confirmed (idempotent)
                let rows = sqlx::query!(
                    r#"
                    UPDATE orders
                    SET payment_status = 'paid'::payment_status,
                        order_status   = 'confirmed'::order_status,
                        updated_at     = now()
                    WHERE id                = $1
                      AND payment_reference = $2
                      AND payment_status    = 'pending'::payment_status
                    "#,
                    order_id,
                    intent_id,
                )
                .execute(&state.db)
                .await
                .map_err(AppError::Database)?
                .rows_affected();

                tracing::info!(order_id = %order_id, "Payment succeeded");

                // Only send emails if the row was actually updated
                // (guards against duplicate webhook delivery)
                if rows > 0 {
                    // Fetch order + farmer details for emails
                    let info = sqlx::query!(
                        r#"
                        SELECT
                            o.guest_email,
                            o.order_key,
                            o.total_amount_gbp,
                            p.name  AS product_name,
                            f.email AS farmer_email
                        FROM orders o
                        JOIN stock    s ON s.id = o.stock_id
                        JOIN products p ON p.id = s.product_id
                        JOIN farmers  f ON f.id = p.farmer_id
                        WHERE o.id = $1
                        "#,
                        order_id,
                    )
                    .fetch_optional(&state.db)
                    .await
                    .map_err(AppError::Database)?;

                    if let Some(info) = info {
                        // Customer payment confirmation email
                        let mailer1        = state.mailer.clone();
                        let customer_email = info.guest_email.clone();
                        let ok1            = info.order_key.clone();
                        let product1       = info.product_name.clone();
                        let total1         = info.total_amount_gbp.to_string();

                        tokio::spawn(async move {
                            let _ = mailer1.send(
                                &customer_email,
                                EmailTemplate::OrderPlaced {
                                    order_key:        ok1,
                                    guest_email:      customer_email.clone(),
                                    total_amount_gbp: total1,
                                    product_name:     product1,
                                },
                            ).await;
                        });

                        // Farmer new order notification email
                        let mailer2      = state.mailer.clone();
                        let farmer_email = info.farmer_email.clone();
                        let ok2          = info.order_key.clone();
                        let product2     = info.product_name.clone();
                        let total2       = info.total_amount_gbp.to_string();
                        let customer2    = info.guest_email.clone();

                        tokio::spawn(async move {
                            let _ = mailer2.send(
                                &farmer_email,
                                EmailTemplate::FarmerNewOrder {
                                    order_key:        ok2,
                                    customer_email:   customer2,
                                    product_name:     product2,
                                    total_amount_gbp: total2,
                                },
                            ).await;
                        });
                    }
                }
            }
        }

        "payment_intent.payment_failed" => {
            if let Ok(order_id) = Uuid::parse_str(order_id_str) {
                sqlx::query!(
                    r#"
                    UPDATE orders
                    SET payment_status = 'failed'::payment_status,
                        updated_at     = now()
                    WHERE id                = $1
                      AND payment_reference = $2
                    "#,
                    order_id,
                    intent_id,
                )
                .execute(&state.db)
                .await
                .map_err(AppError::Database)?;

                tracing::warn!(order_id = %order_id, "Payment failed");
            }
        }

        _ => {
            tracing::debug!(event_type, "Unhandled Stripe webhook event");
        }
    }

    Ok(StatusCode::OK)
}

// ── Stripe HTTP helpers ─────────────────────────────────────────────────────

struct StripeIntent {
    id:            String,
    client_secret: String,
    amount:        i64,
}

async fn stripe_create_intent(
    secret_key:   &str,
    amount_pence: i64,
    order_key:    &str,
    order_id:     Uuid,
) -> AppResult<StripeIntent> {
    let client = reqwest::Client::new();

    let params = [
        ("amount",                 amount_pence.to_string()),
        ("currency",               "gbp".into()),
        ("payment_method_types[]", "card".into()),
        ("metadata[order_id]",     order_id.to_string()),
        ("metadata[order_key]",    order_key.to_string()),
    ];

    let response = client
        .post("https://api.stripe.com/v1/payment_intents")
        .basic_auth(secret_key, Some(""))
        .header("Idempotency-Key", order_key)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stripe request failed: {e}")))?;

    if !response.status().is_success() {
        let err: serde_json::Value = response.json().await.unwrap_or_default();
        return Err(AppError::PaymentFailed(
            err["error"]["message"]
                .as_str()
                .unwrap_or("Stripe error")
                .into(),
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stripe parse failed: {e}")))?;

    Ok(StripeIntent {
        id:            body["id"].as_str().unwrap_or("").into(),
        client_secret: body["client_secret"].as_str().unwrap_or("").into(),
        amount:        body["amount"].as_i64().unwrap_or(0),
    })
}

async fn stripe_get_intent(secret_key: &str, intent_id: &str) -> AppResult<StripeIntent> {
    let client = reqwest::Client::new();

    let response = client
        .get(format!("https://api.stripe.com/v1/payment_intents/{intent_id}"))
        .basic_auth(secret_key, Some(""))
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stripe request failed: {e}")))?;

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stripe parse failed: {e}")))?;

    Ok(StripeIntent {
        id:            body["id"].as_str().unwrap_or("").into(),
        client_secret: body["client_secret"].as_str().unwrap_or("").into(),
        amount:        body["amount"].as_i64().unwrap_or(0),
    })
}

// ── Webhook signature verification ─────────────────────────────────────────

fn verify_stripe_signature(body: &[u8], sig_header: &str, webhook_secret: &str) -> AppResult<()> {
    let mut timestamp = "";
    let mut signature = "";

    for part in sig_header.split(',') {
        if let Some(t) = part.strip_prefix("t=") { timestamp = t; }
        if let Some(s) = part.strip_prefix("v1=") { signature = s; }
    }

    if timestamp.is_empty() || signature.is_empty() {
        return Err(AppError::WebhookSignatureInvalid);
    }

    let signed_payload = format!("{}.{}", timestamp, std::str::from_utf8(body).unwrap_or(""));

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(webhook_secret.as_bytes())
        .map_err(|_| AppError::WebhookSignatureInvalid)?;
    mac.update(signed_payload.as_bytes());

    let expected = hex::encode(mac.finalize().into_bytes());

    if expected != signature {
        return Err(AppError::WebhookSignatureInvalid);
    }

    Ok(())
}