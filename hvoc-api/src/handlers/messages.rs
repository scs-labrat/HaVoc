use std::sync::Arc;
use axum::{extract::State, Json};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub recipient_id: String,
    pub body: String,
}

#[derive(Deserialize)]
pub struct ListMessagesQuery {
    /// If set, list messages for a specific conversation with this peer.
    pub peer_id: Option<String>,
}

pub async fn list_messages(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<ListMessagesQuery>,
) -> Json<serde_json::Value> {
    let author_id = state.author_id.read().await;
    let my_id = match author_id.as_ref() {
        Some(id) => id.clone(),
        None => return Json(serde_json::json!({ "error": "no active identity" })),
    };
    drop(author_id);

    let repo = hvoc_store::MessageRepo(&state.store);

    let messages = if let Some(peer_id) = q.peer_id {
        match repo.list_for_conversation(&my_id, &peer_id).await {
            Ok(msgs) => msgs,
            Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
        }
    } else {
        match repo.list_all_for_user(&my_id).await {
            Ok(msgs) => msgs,
            Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
        }
    };

    Json(serde_json::json!({ "messages": messages }))
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendMessageRequest>,
) -> Json<serde_json::Value> {
    let author_id_guard = state.author_id.read().await;
    let my_id = match author_id_guard.as_ref() {
        Some(id) => id.clone(),
        None => return Json(serde_json::json!({ "error": "no active identity" })),
    };
    drop(author_id_guard);

    let now = chrono::Utc::now().timestamp();
    let object_id = hvoc_core::canon::content_id(&serde_json::json!({
        "sender": my_id,
        "recipient": req.recipient_id,
        "body": req.body,
        "sent_at": now,
    }));

    let object_id = match object_id {
        Ok(id) => id,
        Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
    };

    // Store locally as sent message.
    let repo = hvoc_store::MessageRepo(&state.store);
    let raw_envelope = serde_json::json!({
        "object_id": object_id,
        "sender_id": my_id,
        "recipient_id": req.recipient_id,
        "body": req.body,
        "sent_at": now,
    }).to_string();

    if let Err(e) = repo
        .insert(
            &object_id,
            &my_id,
            &req.recipient_id,
            &req.body,
            now,
            None,
            "sent",
            &raw_envelope,
        )
        .await
    {
        return Json(serde_json::json!({ "error": e.to_string() }));
    }

    // Auto-add contact.
    let contact_repo = hvoc_store::ContactRepo(&state.store);
    let _ = contact_repo.upsert(&req.recipient_id, None).await;

    // Encrypt and attempt delivery via Veilid AppMessage in background.
    let state_bg = state.clone();
    let recipient_id = req.recipient_id.clone();
    let body = req.body.clone();
    let obj_id = object_id.clone();
    tokio::spawn(async move {
        deliver_dm(&state_bg, &recipient_id, &body, &obj_id).await;
    });

    Json(serde_json::json!({
        "object_id": object_id,
        "status": "queued",
    }))
}

/// Background task to encrypt and deliver a DM via Veilid AppMessage.
async fn deliver_dm(state: &AppState, recipient_id: &str, body: &str, _object_id: &str) {
    let kp_guard = state.keypair.read().await;
    let kp = match kp_guard.as_ref().cloned() {
        Some(kp) => kp,
        None => return,
    };
    drop(kp_guard);

    let recipient_pub = match recipient_id.parse::<veilid_core::PublicKey>() {
        Ok(pk) => pk,
        Err(_) => return,
    };

    let encrypted = match state.node.with_crypto(|cs| {
        hvoc_veilid::crypto::encrypt_dm(cs, &kp, &recipient_pub, body)
    }) {
        Ok(e) => e,
        Err(_) => return,
    };

    let ks = hvoc_store::Keystore(&state.store);
    let inbox_key = format!("inbox:{}", recipient_id);
    let (route_key_str, _) = match ks.get_dht_key(&inbox_key).await {
        Ok(Some(v)) => v,
        _ => return,
    };

    let rc = match state.node.routing_context() {
        Ok(rc) => rc,
        Err(_) => return,
    };

    let route_key = match route_key_str.parse::<veilid_core::RecordKey>() {
        Ok(k) => k,
        Err(_) => return,
    };

    let _ = hvoc_veilid::dht::open_record_readonly(&rc, route_key.clone()).await;
    if let Ok(Some(route_blob)) = hvoc_veilid::dht::get_value(&rc, route_key.clone(), 0, true).await {
        if let Ok(route_id) = state.node.api.import_remote_private_route(route_blob) {
            let msg_bytes = serde_json::to_vec(&encrypted).unwrap_or_default();
            let _ = rc.app_message(veilid_core::Target::RouteId(route_id), msg_bytes).await;
        }
    }
    let _ = hvoc_veilid::dht::close_record(&rc, route_key).await;
}

#[derive(Deserialize)]
pub struct AddContactRequest {
    pub author_id: String,
    pub nickname: Option<String>,
}

pub async fn add_contact(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddContactRequest>,
) -> Json<serde_json::Value> {
    let repo = hvoc_store::ContactRepo(&state.store);
    if let Err(e) = repo.upsert(&req.author_id, req.nickname.as_deref()).await {
        return Json(serde_json::json!({ "error": e.to_string() }));
    }

    // Try to fetch their profile from DHT in background.
    let state_bg = state.clone();
    let author_id = req.author_id.clone();
    tokio::spawn(async move {
        fetch_contact_profile(&state_bg, &author_id).await;
    });

    Json(serde_json::json!({ "status": "ok" }))
}

/// Fetch a contact's profile from DHT and store locally.
async fn fetch_contact_profile(state: &AppState, author_id: &str) {
    let ks = hvoc_store::Keystore(&state.store);
    let logical_key = format!("profile:{}", author_id);

    // Check if we have a DHT key for this profile.
    let record_key_str = match ks.get_dht_key(&logical_key).await {
        Ok(Some((rk, _))) => rk,
        _ => return, // No known DHT key for this profile yet.
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
    if let Ok(Some(data)) = hvoc_veilid::dht::fetch_profile(&rc, record_key.clone()).await {
        if let Ok(profile) = serde_json::from_slice::<hvoc_core::Profile>(&data) {
            let repo = hvoc_store::IdentityRepo(&state.store);
            let raw_json = serde_json::to_string(&profile).unwrap_or_default();
            let _ = repo.upsert(
                &profile.author_id,
                &profile.handle,
                &profile.bio,
                &profile.author_id,
                &raw_json,
            ).await;
            tracing::info!("Fetched profile for contact: {}", &profile.handle);
        }
    }
    let _ = hvoc_veilid::dht::close_record(&rc, record_key).await;
}

pub async fn list_contacts(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let repo = hvoc_store::ContactRepo(&state.store);
    match repo.list().await {
        Ok(contacts) => Json(serde_json::json!({ "contacts": contacts })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
