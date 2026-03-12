use std::sync::Arc;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Serialize)]
pub struct IdentityResponse {
    pub author_id: String,
    pub handle: String,
}

#[derive(Deserialize)]
pub struct CreateIdentityRequest {
    pub handle: String,
    pub passphrase: String,
}

#[derive(Deserialize)]
pub struct UnlockIdentityRequest {
    pub author_id: String,
    pub passphrase: String,
}

pub async fn get_identity(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let author_id = state.author_id.read().await;
    match author_id.as_ref() {
        Some(id) => Json(serde_json::json!({ "author_id": id })),
        None => Json(serde_json::json!({ "error": "no active identity" })),
    }
}

pub async fn create_identity(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateIdentityRequest>,
) -> Json<serde_json::Value> {
    let result = state.node.with_crypto(|cs| {
        let kp = hvoc_veilid::crypto::generate_keypair(cs);
        let author_id = hvoc_veilid::crypto::author_id_from_key(&kp.key());
        Ok((kp, author_id))
    });

    let (kp, author_id) = match result {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };

    // Save encrypted keypair (public + secret) to keystore.
    let ks = hvoc_store::Keystore(&state.store);
    let mut keypair_bytes = Vec::with_capacity(64);
    keypair_bytes.extend_from_slice(kp.key().value().bytes());
    keypair_bytes.extend_from_slice(kp.secret().value().bytes());
    if let Err(e) = ks
        .save(&author_id, &req.handle, &keypair_bytes, req.passphrase.as_bytes())
        .await
    {
        return Json(serde_json::json!({ "error": e.to_string() }));
    }

    // Set as active identity.
    *state.keypair.write().await = Some(kp);
    *state.author_id.write().await = Some(author_id.clone());

    // Publish profile and set up inbox route in background.
    let state_clone = state.clone();
    tokio::spawn(async move {
        super::profile::publish_profile(&state_clone).await;
        setup_inbox_route(&state_clone).await;
    });

    Json(serde_json::json!({
        "author_id": author_id,
        "handle": req.handle,
    }))
}

/// Unlock a previously-created identity by decrypting the secret from keystore.
pub async fn unlock_identity(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UnlockIdentityRequest>,
) -> Json<serde_json::Value> {
    let ks = hvoc_store::Keystore(&state.store);

    // Load and decrypt the keypair bytes (32 public + 32 secret).
    let keypair_bytes = match ks.load(&req.author_id, req.passphrase.as_bytes()).await {
        Ok(bytes) => bytes,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };

    if keypair_bytes.len() != 64 {
        return Json(serde_json::json!({ "error": "corrupt keypair data (expected 64 bytes)" }));
    }

    // Reconstruct the keypair from stored bytes.
    let kp = {
        use std::convert::TryFrom;
        let bare_pub = match veilid_core::BarePublicKey::try_from(&keypair_bytes[..32]) {
            Ok(k) => k,
            Err(e) => return Json(serde_json::json!({ "error": format!("invalid public key: {e}") })),
        };
        let bare_secret = match veilid_core::BareSecretKey::try_from(&keypair_bytes[32..]) {
            Ok(k) => k,
            Err(e) => return Json(serde_json::json!({ "error": format!("invalid secret key: {e}") })),
        };
        let bare_kp = veilid_core::BareKeyPair::new(bare_pub, bare_secret);
        veilid_core::KeyPair::new(veilid_core::CRYPTO_KIND_VLD0, bare_kp)
    };

    let author_id = hvoc_veilid::crypto::author_id_from_key(&kp.key());

    *state.keypair.write().await = Some(kp);
    *state.author_id.write().await = Some(author_id.clone());

    // Set up inbox route in background.
    let state_clone = state.clone();
    tokio::spawn(async move {
        setup_inbox_route(&state_clone).await;
    });

    Json(serde_json::json!({
        "author_id": author_id,
        "status": "unlocked",
    }))
}

pub async fn list_identities(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let ks = hvoc_store::Keystore(&state.store);
    match ks.list_ids().await {
        Ok(ids) => {
            let list: Vec<_> = ids
                .iter()
                .map(|i| serde_json::json!({ "id": i.id, "handle": i.handle }))
                .collect();
            Json(serde_json::json!({ "identities": list }))
        }
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// Create a private route for receiving DMs and publish it to DHT.
async fn setup_inbox_route(state: &AppState) {
    let author_id_guard = state.author_id.read().await;
    let author_id = match author_id_guard.as_ref() {
        Some(id) => id.clone(),
        None => return,
    };
    drop(author_id_guard);

    // Create a new private route.
    let route_blob = match state.node.api.new_private_route().await {
        Ok(blob) => blob,
        Err(e) => {
            tracing::warn!("Failed to create private route for inbox: {e}");
            return;
        }
    };

    // Publish the route blob to a DHT record.
    let rc = match state.node.routing_context() {
        Ok(rc) => rc,
        Err(_) => return,
    };

    let ks = hvoc_store::Keystore(&state.store);
    let logical_key = format!("inbox:{}", author_id);

    // Check if we already have an inbox DHT record.
    let existing = match ks.get_dht_key(&logical_key).await {
        Ok(v) => v,
        Err(_) => return,
    };

    let record_key = if let Some((record_key_str, owner_secret)) = existing {
        // Existing record — need to open it for writing.
        let record_key = match record_key_str.parse::<veilid_core::RecordKey>() {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Failed to parse inbox record key: {e}");
                return;
            }
        };
        let writer = match owner_secret {
            Some(ref s) => match s.parse::<veilid_core::KeyPair>() {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to parse inbox owner secret: {e}");
                    return;
                }
            },
            None => {
                tracing::warn!("No owner secret for inbox record, cannot publish");
                return;
            }
        };
        if let Err(e) = hvoc_veilid::dht::open_record_writable(&rc, record_key.clone(), writer).await {
            tracing::warn!("Failed to open inbox record for writing: {e}");
            return;
        }
        record_key
    } else {
        // Create a new inbox DHT record (create_record also opens it).
        let schema = veilid_core::DHTSchema::dflt(1).unwrap();
        match hvoc_veilid::dht::create_record(&rc, schema, None).await {
            Ok((rk, okp)) => {
                let secret = okp.map(|kp| kp.to_string());
                let _ = ks.save_dht_key(&logical_key, &rk.to_string(), secret.as_deref()).await;
                rk
            }
            Err(e) => {
                tracing::warn!("Failed to create inbox DHT record: {e}");
                return;
            }
        }
    };

    // Publish the route blob.
    if let Err(e) = hvoc_veilid::dht::publish_inbox(&rc, record_key.clone(), &route_blob.blob).await {
        tracing::warn!("Failed to publish inbox route blob: {e}");
    } else {
        tracing::info!("Inbox route published for {}", &author_id[..12.min(author_id.len())]);
    }
    let _ = hvoc_veilid::dht::close_record(&rc, record_key).await;
}
