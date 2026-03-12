//! Background sync tasks: board index, DHT reconciliation, incoming DM handling.

use std::sync::Arc;
use crate::AppState;

/// Board index DHT record key (well-known, shared by all nodes).
/// In a real deployment this would be configurable or derived from a seed.
const BOARD_LOGICAL_KEY: &str = "board:default";

/// Register a thread in the board index DHT record so other nodes can discover it.
pub async fn register_thread_in_board(state: &AppState, thread_id: &str, thread_dht_key: &str) {
    // Store locally.
    let board = hvoc_store::BoardRepo(&state.store);
    let _ = board.add_thread("default", thread_dht_key, thread_id).await;

    // Update the board index DHT record.
    let rc = match state.node.routing_context() {
        Ok(rc) => rc,
        Err(_) => return,
    };

    let ks = hvoc_store::Keystore(&state.store);
    let (record_key_str, owner_secret) = match ks.get_dht_key(BOARD_LOGICAL_KEY).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            // First time: create the board index DHT record.
            let schema = veilid_core::DHTSchema::dflt(1).unwrap();
            match hvoc_veilid::dht::create_record(&rc, schema, None).await {
                Ok((rk, okp)) => {
                    let secret = okp.map(|kp| kp.to_string());
                    let _ = ks.save_dht_key(BOARD_LOGICAL_KEY, &rk.to_string(), secret.as_deref()).await;
                    (rk.to_string(), secret)
                }
                Err(e) => {
                    tracing::warn!("Failed to create board index DHT record: {e}");
                    return;
                }
            }
        }
        Err(_) => return,
    };

    if let Ok(record_key) = record_key_str.parse::<veilid_core::RecordKey>() {
        // Open writable if we have the owner secret.
        if let Some(ref s) = owner_secret {
            if let Ok(writer) = s.parse::<veilid_core::KeyPair>() {
                let _ = hvoc_veilid::dht::open_record_writable(&rc, record_key.clone(), writer).await;
            }
        }

        // Read current board index, append new entry.
        let mut entries: Vec<serde_json::Value> = Vec::new();
        if let Ok(Some(data)) = hvoc_veilid::dht::get_value(&rc, record_key.clone(), 0, false).await {
            if let Ok(existing) = serde_json::from_slice::<Vec<serde_json::Value>>(&data) {
                entries = existing;
            }
        }

        // Avoid duplicates.
        let already = entries.iter().any(|e| e.get("thread_id").and_then(|v| v.as_str()) == Some(thread_id));
        if !already {
            entries.push(serde_json::json!({
                "thread_id": thread_id,
                "dht_key": thread_dht_key,
            }));
            if let Ok(json) = serde_json::to_vec(&entries) {
                let _ = hvoc_veilid::dht::set_value(&rc, record_key.clone(), 0, json).await;
            }
        }

        let _ = hvoc_veilid::dht::close_record(&rc, record_key).await;
    }
}

/// Reconcile on startup: pull threads from the board index DHT and store locally.
pub async fn reconcile_from_dht(state: &Arc<AppState>) {
    let ks = hvoc_store::Keystore(&state.store);

    // Try to fetch the board index.
    let record_key_str = match ks.get_dht_key(BOARD_LOGICAL_KEY).await {
        Ok(Some((rk, _))) => rk,
        _ => {
            tracing::info!("No board index DHT key found, skipping reconciliation");
            return;
        }
    };

    let rc = match state.node.routing_context() {
        Ok(rc) => rc,
        Err(_) => return,
    };

    let record_key = match record_key_str.parse::<veilid_core::RecordKey>() {
        Ok(k) => k,
        Err(_) => return,
    };

    let _ = hvoc_veilid::dht::open_record_readonly(&rc, record_key.clone()).await;

    // Read the board index entries.
    let entries = match hvoc_veilid::dht::get_value(&rc, record_key.clone(), 0, true).await {
        Ok(Some(data)) => {
            serde_json::from_slice::<Vec<serde_json::Value>>(&data).unwrap_or_default()
        }
        _ => Vec::new(),
    };

    tracing::info!("Board index has {} entries", entries.len());

    for entry in &entries {
        let thread_dht_key = match entry.get("dht_key").and_then(|v| v.as_str()) {
            Some(k) => k,
            None => continue,
        };
        let thread_id = match entry.get("thread_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        // Check if we already have this thread locally.
        let thread_repo = hvoc_store::ThreadRepo(&state.store);
        if thread_repo.get(thread_id).await.is_ok() {
            continue;
        }

        // Fetch thread from DHT.
        if let Ok(dht_key) = thread_dht_key.parse::<veilid_core::RecordKey>() {
            let _ = hvoc_veilid::dht::open_record_readonly(&rc, dht_key.clone()).await;

            // Fetch thread header (subkey 0).
            if let Ok(Some(data)) = hvoc_veilid::dht::get_value(&rc, dht_key.clone(), 0, true).await {
                if let Ok(thread) = serde_json::from_slice::<hvoc_core::Thread>(&data) {
                    let _ = thread_repo.insert(&thread).await;
                    tracing::info!("Reconciled thread: {} ({})", thread.title, thread.object_id);

                    // Fetch post index (subkey 1).
                    if let Ok(Some(index_data)) = hvoc_veilid::dht::get_value(&rc, dht_key.clone(), 1, true).await {
                        if let Ok(post_ids) = serde_json::from_slice::<Vec<String>>(&index_data) {
                            // Fetch each post.
                            for post_id in &post_ids {
                                let post_logical = format!("post:{}", post_id);
                                // We need to find the post's DHT key — check if it's in our board entries.
                                // For now, skip individual post fetch since we'd need a separate post registry.
                                let _ = post_logical; // Acknowledge we can't resolve this yet.
                            }

                            // Update post count.
                            let _ = thread_repo.increment_post_count(thread_id, chrono::Utc::now().timestamp()).await;
                        }
                    }
                }
            }

            let _ = hvoc_veilid::dht::close_record(&rc, dht_key).await;
        }

        // Store in local board index.
        let board = hvoc_store::BoardRepo(&state.store);
        let _ = board.add_thread("default", thread_dht_key, thread_id).await;
    }

    // Watch the board index for future changes.
    let _ = hvoc_veilid::dht::watch_record(&rc, record_key.clone()).await;
    let _ = hvoc_veilid::dht::close_record(&rc, record_key).await;
}

/// Handle an incoming AppMessage (encrypted DM).
pub async fn handle_incoming_dm(state: &Arc<AppState>, payload: &[u8]) {
    // Try to deserialize as an EncryptedDm.
    let envelope: hvoc_veilid::crypto::EncryptedDm = match serde_json::from_slice(payload) {
        Ok(e) => e,
        Err(_) => return, // Not a DM, ignore.
    };

    let kp_guard = state.keypair.read().await;
    let kp = match kp_guard.as_ref() {
        Some(kp) => kp.clone(),
        None => return,
    };
    drop(kp_guard);

    // Parse sender public key.
    let sender_pub = match envelope.sender_id.parse::<veilid_core::PublicKey>() {
        Ok(k) => k,
        Err(_) => return,
    };

    // Decrypt.
    let dm_payload = match state.node.with_crypto(|cs| {
        hvoc_veilid::crypto::decrypt_dm(cs, &kp, &sender_pub, &envelope)
    }) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to decrypt incoming DM: {e}");
            return;
        }
    };

    // Store decrypted message.
    let my_id = hvoc_veilid::crypto::author_id_from_key(&kp.key());
    let object_id = format!("dm-{}-{}", envelope.sender_id, envelope.sent_at);

    let repo = hvoc_store::MessageRepo(&state.store);
    let _ = repo
        .insert(
            &object_id,
            &envelope.sender_id,
            &my_id,
            &dm_payload.body,
            dm_payload.sent_at,
            Some(chrono::Utc::now().timestamp()),
            "received",
            &serde_json::to_string(&envelope).unwrap_or_default(),
        )
        .await;

    // Auto-add sender as contact.
    let contact_repo = hvoc_store::ContactRepo(&state.store);
    let _ = contact_repo.upsert(&envelope.sender_id, None).await;

    tracing::info!("Received DM from {}", &envelope.sender_id[..12.min(envelope.sender_id.len())]);
}
