use serde::{Deserialize, Serialize};

/// Every event sent over the WebSocket uses this envelope.
/// The frontend switches on `event_type` to decide how to handle it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent {
    /// e.g. "order_placed", "stock_low", "payment_received"
    pub event_type: String,

    /// The farmer this event is addressed to.
    /// None = broadcast to all connected farmers (admin use).
    pub farmer_id: Option<String>,

    /// Arbitrary JSON payload — each event type defines its own shape.
    pub payload: serde_json::Value,
}

impl WsEvent {
    pub fn new(
        event_type: impl Into<String>,
        farmer_id:  Option<String>,
        payload:    serde_json::Value,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            farmer_id,
            payload,
        }
    }

    pub fn for_farmer(
        event_type: impl Into<String>,
        farmer_id:  &str,
        payload:    serde_json::Value,
    ) -> Self {
        Self::new(event_type, Some(farmer_id.to_string()), payload)
    }
}