//! Sync events derived from Veilid updates.

use serde::{Deserialize, Serialize};

/// High-level events that the application reacts to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncEvent {
    /// A watched DHT value changed on the network.
    DhtValueChanged {
        record_key: String,
        subkeys: Vec<u32>,
    },

    /// An AppMessage arrived (typically an encrypted DM).
    AppMessageReceived {
        sender: Option<String>,
        payload: Vec<u8>,
    },

    /// The node's attachment state changed.
    AttachmentChanged {
        state: String,
        public_internet_ready: bool,
    },

    /// A private route we depend on has died and needs recreation.
    RouteDied {
        dead_routes: Vec<String>,
    },
}
