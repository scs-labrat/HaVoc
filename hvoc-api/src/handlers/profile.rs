use std::sync::Arc;
use axum::{extract::State, Json};

use crate::AppState;

/// Get a handle/profile for a given author_id.
pub async fn get_profile(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(author_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let repo = hvoc_store::IdentityRepo(&state.store);
    match repo.get(&author_id).await {
        Ok(Some(identity)) => Json(serde_json::json!({
            "author_id": identity.author_id,
            "handle": identity.handle,
            "bio": identity.bio,
        })),
        Ok(None) => Json(serde_json::json!({ "error": "not found" })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// Resolve handles for multiple author IDs at once.
pub async fn resolve_handles(
    State(state): State<Arc<AppState>>,
    Json(author_ids): Json<Vec<String>>,
) -> Json<serde_json::Value> {
    let repo = hvoc_store::IdentityRepo(&state.store);

    // Also check the keystore for local identities.
    let ks = hvoc_store::Keystore(&state.store);
    let mut handles = match repo.get_handles(&author_ids).await {
        Ok(h) => h,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };

    // Fill in from keystore for local identities not in identities table.
    if let Ok(local_ids) = ks.list_ids().await {
        for id in local_ids {
            if author_ids.contains(&id.id) && !handles.contains_key(&id.id) {
                handles.insert(id.id, id.handle);
            }
        }
    }

    Json(serde_json::json!({ "handles": handles }))
}

/// Publish the active identity's profile to DHT.
pub async fn publish_profile(state: &AppState) {
    let kp_guard = state.keypair.read().await;
    let kp = match kp_guard.as_ref() {
        Some(kp) => kp.clone(),
        None => return,
    };
    drop(kp_guard);

    let author_id_guard = state.author_id.read().await;
    let author_id = match author_id_guard.as_ref() {
        Some(id) => id.clone(),
        None => return,
    };
    drop(author_id_guard);

    // Get handle from keystore.
    let ks = hvoc_store::Keystore(&state.store);
    let ids = match ks.list_ids().await {
        Ok(ids) => ids,
        Err(_) => return,
    };
    let handle = ids.iter()
        .find(|i| i.id == author_id)
        .map(|i| i.handle.clone())
        .unwrap_or_else(|| author_id.clone());

    // Create signed profile.
    let profile = match state.node.with_crypto(|cs| {
        hvoc_veilid::crypto::create_profile(cs, &kp, &handle, "")
    }) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to create profile: {e}");
            return;
        }
    };

    // Store locally in identities table.
    let repo = hvoc_store::IdentityRepo(&state.store);
    let raw_json = serde_json::to_string(&profile).unwrap_or_default();
    let _ = repo.upsert(&author_id, &handle, "", &kp.key().to_string(), &raw_json).await;

    // Publish to DHT.
    let rc = match state.node.routing_context() {
        Ok(rc) => rc,
        Err(_) => return,
    };

    let schema = veilid_core::DHTSchema::dflt(1).unwrap();
    match hvoc_veilid::dht::create_record(&rc, schema, None).await {
        Ok((record_key, owner_kp)) => {
            let ks = hvoc_store::Keystore(&state.store);
            let logical_key = format!("profile:{}", author_id);
            let owner_secret = owner_kp.map(|kp| kp.to_string());
            let _ = ks.save_dht_key(&logical_key, &record_key.to_string(), owner_secret.as_deref()).await;

            if let Ok(json) = serde_json::to_vec(&profile) {
                let _ = hvoc_veilid::dht::publish_profile(&rc, record_key.clone(), &json).await;
            }
            let _ = hvoc_veilid::dht::close_record(&rc, record_key).await;
        }
        Err(e) => tracing::warn!("Failed to create profile DHT record: {e}"),
    }
}
