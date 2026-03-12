#![recursion_limit = "256"]
//! HTTP/WebSocket API bridge for the HVOC frontend.
//!
//! Runs on localhost and exposes REST + WebSocket endpoints that the
//! browser UI (hvoc.html) connects to instead of using IndexedDB.

pub mod handlers;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use hvoc_store::Store;
use hvoc_veilid::HvocNode;

/// Shared application state.
pub struct AppState {
    pub store: Store,
    pub node: Arc<HvocNode>,
    /// Active identity keypair (set after login).
    pub keypair: RwLock<Option<veilid_core::KeyPair>>,
    /// Author ID string of the active identity.
    pub author_id: RwLock<Option<String>>,
    /// Data directory path (for attachments, etc.).
    pub data_dir: std::path::PathBuf,
}

/// Embedded frontend HTML (bundled at compile time).
const FRONTEND_HTML: &str = include_str!("../../hvoc.html");

/// Serve the frontend with URLs rewritten to be relative (works on any port).
async fn serve_frontend() -> impl IntoResponse {
    let html = FRONTEND_HTML
        .replace(
            "const API_BASE = 'http://127.0.0.1:7734'",
            "const API_BASE = window.location.origin",
        )
        .replace(
            "`ws://127.0.0.1:7734/ws`",
            "`${window.location.origin.replace('http','ws')}/ws`",
        );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
}

/// Start the API server.
pub async fn serve(state: Arc<AppState>, addr: SocketAddr) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Frontend
        .route("/", get(serve_frontend))
        // Identity
        .route("/api/identity", get(handlers::identity::get_identity).post(handlers::identity::create_identity))
        .route("/api/identity/unlock", post(handlers::identity::unlock_identity))
        .route("/api/identity/list", get(handlers::identity::list_identities))
        // Forum
        .route("/api/threads", get(handlers::forum::list_threads).post(handlers::forum::create_thread))
        .route("/api/threads/:id", get(handlers::forum::get_thread).delete(handlers::forum::delete_thread))
        .route("/api/threads/:id/posts", get(handlers::forum::list_posts).post(handlers::forum::create_post))
        .route("/api/posts/:id", delete(handlers::forum::delete_post))
        // Messages
        .route("/api/messages", get(handlers::messages::list_messages).post(handlers::messages::send_message))
        .route("/api/contacts", get(handlers::messages::list_contacts).post(handlers::messages::add_contact))
        // Attachments
        .route("/api/attachments", post(handlers::attachments::upload))
        .route("/api/attachments/:hash", get(handlers::attachments::serve_file))
        // Board bootstrap info
        .route("/api/board/info", get(handlers::board::get_board_info))
        // Profiles / handle resolution
        .route("/api/profiles/:author_id", get(handlers::profile::get_profile))
        .route("/api/profiles/resolve", post(handlers::profile::resolve_handles))
        // WebSocket for live updates
        .route("/ws", get(handlers::ws::ws_handler))
        .layer(cors)
        .with_state(state.clone());

    // Spawn board bootstrap + reconciliation.
    let sync_state = state.clone();
    tokio::spawn(async move {
        // Wait a bit for the node to attach.
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        // Ensure board index exists (DNS/env/local/create).
        handlers::board::ensure_board_index(&sync_state).await;
        // Then reconcile threads from it.
        handlers::sync::reconcile_from_dht(&sync_state).await;
    });

    // Spawn incoming DM handler.
    let dm_state = state.clone();
    tokio::spawn(async move {
        let mut rx = dm_state.node.subscribe_sync();
        while let Ok(event) = rx.recv().await {
            if let hvoc_veilid::SyncEvent::AppMessageReceived { payload, .. } = event {
                handlers::sync::handle_incoming_dm(&dm_state, &payload).await;
            }
        }
    });

    tracing::info!("HVOC API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
