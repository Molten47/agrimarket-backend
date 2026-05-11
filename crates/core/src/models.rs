use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ── FARMERS ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Farmer {
    pub id:           Uuid,
    pub email:        String,
    #[serde(skip_serializing)]  // never expose hash in API responses
    pub password_hash: String,
    pub farm_name:    String,
    pub full_name:    String,
    pub phone:        Option<String>,
    pub county:       String,
    pub postcode:     String,
    pub bio:          Option<String>,
    pub is_active:    bool,
    pub created_at:   DateTime<Utc>,
    pub updated_at:   DateTime<Utc>,
}

// Safe public projection — never include password_hash
#[derive(Debug, Serialize, Deserialize)]
pub struct FarmerPublic {
    pub id:        Uuid,
    pub email:     String,
    pub farm_name: String,
    pub full_name: String,
    pub county:    String,
    pub postcode:  String,
    pub bio:       Option<String>,
    pub is_active: bool,
}

impl From<Farmer> for FarmerPublic {
    fn from(f: Farmer) -> Self {
        Self {
            id:        f.id,
            email:     f.email,
            farm_name: f.farm_name,
            full_name: f.full_name,
            county:    f.county,
            postcode:  f.postcode,
            bio:       f.bio,
            is_active: f.is_active,
        }
    }
}

// ── REFRESH TOKENS ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RefreshToken {
    pub id:         Uuid,
    pub farmer_id:  Uuid,
    pub token_hash: String,
    pub family_id:  Uuid,   // all tokens in a rotation chain share this
    pub is_revoked: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

// ── CATEGORIES ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Category {
    pub id:          Uuid,
    pub name:        String,
    pub slug:        String,
    pub parent_id:   Option<Uuid>,   // self-referencing — null = top-level
    pub description: Option<String>,
    pub is_active:   bool,
}

// ── PRODUCTS ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Product {
    pub id:              Uuid,
    pub farmer_id:       Uuid,
    pub category_id:     Uuid,
    pub name:            String,
    pub slug:            String,
    pub description:     Option<String>,
    pub price_per_unit:  rust_decimal::Decimal,   // numeric(10,2) GBP
    pub unit:            String,   // e.g. "kg", "dozen", "bunch"
    pub is_active:       bool,
    pub is_deleted:      bool,   // soft delete
    pub created_at:      DateTime<Utc>,
    pub updated_at:      DateTime<Utc>,
}

// ── STOCK ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Stock {
    pub id:                  Uuid,
    pub product_id:          Uuid,
    pub quantity_available:  rust_decimal::Decimal,
    pub quantity_reserved:   rust_decimal::Decimal,
    pub low_stock_threshold: rust_decimal::Decimal,
    pub stock_status:        StockStatus,
    pub updated_at:          DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, sqlx::Type)]
#[sqlx(type_name = "stock_status", rename_all = "snake_case")]
pub enum StockStatus {
    InStock,
    LowStock,
    OutOfStock,
}

// ── CART ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Cart {
    pub id:          Uuid,
    pub session_key: String,   // anonymous guest session
    pub expires_at:  DateTime<Utc>,
    pub created_at:  DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct CartItem {
    pub id:         Uuid,
    pub cart_id:    Uuid,
    pub product_id: Uuid,
    pub quantity:   rust_decimal::Decimal,
    pub added_at:   DateTime<Utc>,
}

// ── ORDERS ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Order {
    pub id:               Uuid,
    pub stock_id:         Uuid,
    pub order_key:        String,   // client-generated, UK constraint prevents duplicates
    pub guest_email:      String,
    pub guest_phone:      Option<String>,
    pub delivery_address: String,
    pub delivery_county:  String,
    pub delivery_postcode: String,
    pub order_status:     OrderStatus,
    pub payment_status:   PaymentStatus,
    pub payment_method:   Option<String>,
    pub payment_reference: Option<String>,
    pub total_amount_gbp: rust_decimal::Decimal,
    pub placed_at:        DateTime<Utc>,
    pub updated_at:       DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, sqlx::Type)]
#[sqlx(type_name = "order_status", rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Confirmed,
    Processing,
    Dispatched,
    Delivered,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, sqlx::Type)]
#[sqlx(type_name = "payment_status", rename_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Paid,
    Failed,
    Refunded,
}

// ── ORDER ITEMS ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct OrderItem {
    pub id:            Uuid,
    pub order_id:      Uuid,
    pub product_id:    Uuid,
    pub quantity:      rust_decimal::Decimal,
    pub unit_price_gbp: rust_decimal::Decimal,  // price snapshot at time of order
    pub subtotal_gbp:  rust_decimal::Decimal,
}

// ── TRACKING ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct TrackingEvent {
    pub id:             Uuid,
    pub order_id:       Uuid,
    pub status:         String,
    pub location_label: Option<String>,
    pub lat:            Option<rust_decimal::Decimal>,
    pub lng:            Option<rust_decimal::Decimal>,
    pub event_time:     DateTime<Utc>,
}

// ── NOTIFICATIONS ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Notification {
    pub id:              Uuid,
    pub farmer_id:       Option<Uuid>,
    pub order_id:        Option<Uuid>,
    pub channel:         String,         // email | websocket | push
    pub recipient_email: Option<String>,
    pub event_type:      String,
    pub payload:         serde_json::Value,  // jsonb template vars
    pub is_sent:         bool,
    pub sent_at:         Option<DateTime<Utc>>,
    pub created_at:      DateTime<Utc>,
}
