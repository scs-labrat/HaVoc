use std::sync::Arc;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub struct UploadRequest {
    /// Base64-encoded file data.
    pub data: String,
    /// Original filename.
    pub filename: String,
    /// MIME type (e.g. "image/png").
    pub mime: String,
}

/// Upload an attachment. Returns its BLAKE3 hash for referencing.
pub async fn upload(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UploadRequest>,
) -> Json<serde_json::Value> {
    let bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.data) {
        Ok(b) => b,
        Err(e) => return Json(serde_json::json!({ "error": format!("invalid base64: {e}") })),
    };

    let hash = blake3::hash(&bytes).to_hex().to_string();

    let attach_dir = state.data_dir.join("attachments");
    if let Err(e) = std::fs::create_dir_all(&attach_dir) {
        return Json(serde_json::json!({ "error": format!("mkdir failed: {e}") }));
    }

    let file_path = attach_dir.join(&hash);
    if let Err(e) = std::fs::write(&file_path, &bytes) {
        return Json(serde_json::json!({ "error": format!("write failed: {e}") }));
    }

    let meta = serde_json::json!({
        "hash": hash,
        "filename": req.filename,
        "mime": req.mime,
        "size": bytes.len(),
    });

    Json(serde_json::json!({ "attachment": meta }))
}

/// Serve an attachment file by its BLAKE3 hash.
pub async fn serve_file(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Response {
    // Sanitize: only allow hex chars.
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return (StatusCode::BAD_REQUEST, "invalid hash").into_response();
    }

    let file_path = state.data_dir.join("attachments").join(&hash);
    match std::fs::read(&file_path) {
        Ok(data) => {
            // Try to guess content type from first bytes.
            let content_type = guess_mime(&data);
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
                ],
                data,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn guess_mime(data: &[u8]) -> String {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png".into()
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg".into()
    } else if data.starts_with(b"GIF8") {
        "image/gif".into()
    } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
        "image/webp".into()
    } else if data.starts_with(b"%PDF") {
        "application/pdf".into()
    } else {
        "application/octet-stream".into()
    }
}
