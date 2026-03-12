//! WebSocket handler for live sync events.
//!
//! The frontend connects here to receive real-time updates (new posts,
//! DHT value changes, attachment state, etc.) instead of polling.

use std::sync::Arc;
use axum::{
    extract::{State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};

use crate::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to sync events from the Veilid node.
    let mut sync_rx = state.node.subscribe_sync();

    // Forward sync events to the WebSocket client.
    let send_task = tokio::spawn(async move {
        while let Ok(event) = sync_rx.recv().await {
            let json = match serde_json::to_string(&event) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Read messages from the client (for future use: commands, acks, etc.)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                _ => {
                    // TODO: Handle client commands if needed.
                }
            }
        }
    });

    // Wait for either task to finish.
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
