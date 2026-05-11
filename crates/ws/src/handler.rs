use axum::{
    extract::{Query, WebSocketUpgrade, ws::{WebSocket, Message}},
    response::Response,
};
use serde::Deserialize;
use tracing::{info, warn};
use crate::broadcaster::WsBroadcaster;

#[derive(Deserialize)]
pub struct WsQuery {
    /// farmer_id passed as ?farmer_id=<uuid> from the frontend.
    /// Used to filter events — only events addressed to this farmer
    /// (or broadcast events with farmer_id = None) are forwarded.
    pub farmer_id: Option<String>,
}

/// Route: GET /ws?farmer_id=<uuid>
/// Upgrades to WebSocket and streams filtered events to the client.
pub async fn ws_handler(
    ws:       WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    axum::extract::Extension(broadcaster): axum::extract::Extension<WsBroadcaster>,
) -> Response {
    let farmer_id = q.farmer_id.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster, farmer_id))
}

async fn handle_socket(
    mut socket:   WebSocket,
    broadcaster:  WsBroadcaster,
    farmer_id:    Option<String>,
) {
    let mut rx = broadcaster.subscribe();
    info!(farmer_id = ?farmer_id, "WebSocket client connected");

    loop {
        tokio::select! {
            // Incoming event from the broadcaster
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        // Forward only if: broadcast (no farmer_id) OR matches this client
                        let addressed_to_me = ev.farmer_id.is_none()
                            || ev.farmer_id == farmer_id;

                        if !addressed_to_me { continue; }

                        let json = match serde_json::to_string(&ev) {
                            Ok(j)  => j,
                            Err(e) => { warn!("WS serialize error: {e}"); continue; }
                        };

                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break; // client disconnected
                        }
                    }
                    Err(_) => break, // channel lagged or closed
                }
            }

            // Incoming message from the client (ping / close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    _ => {} // ignore text frames from client
                }
            }
        }
    }

    info!(farmer_id = ?farmer_id, "WebSocket client disconnected");
}