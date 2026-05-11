use tokio::sync::broadcast;
use crate::message::WsEvent;

/// Capacity: 256 events buffered.
/// If a slow client falls behind, older events are dropped (not blocked).
const CHANNEL_CAPACITY: usize = 256;

/// Cheap to clone — just clones the sender handle.
/// Store one instance in AppState; clone into each handler that needs to publish.
#[derive(Clone)]
pub struct WsBroadcaster {
    tx: broadcast::Sender<WsEvent>,
}

impl WsBroadcaster {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    /// Publish an event — all connected WebSocket clients receive it.
    /// Returns the number of active receivers at the moment of send.
    /// Errors only if there are zero receivers (safe to ignore).
    pub fn publish(&self, event: WsEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    /// Each WebSocket connection calls this to get its own receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.tx.subscribe()
    }
}

impl Default for WsBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}