//! Board bootstrap: discover the shared board index via DNS TXT record,
//! environment variables, or create a new standalone board.
//!
//! Priority:
//!   1. Env vars HVOC_BOARD_KEY + HVOC_BOARD_SECRET (explicit override)
//!   2. DNS TXT record at HVOC_BOOTSTRAP_DOMAIN (default: _hvoc-board.veilid.org)
//!   3. Existing board key in local keystore
//!   4. Create a new board (standalone mode)

use std::sync::Arc;
use axum::{extract::State, Json};

use crate::AppState;

/// DNS domain to query for the bootstrap board TXT record.
/// Override with HVOC_BOOTSTRAP_DOMAIN env var.
const DEFAULT_BOOTSTRAP_DOMAIN: &str = "_hvoc-board.hvck.academy";

/// DNS-over-HTTPS endpoint (Cloudflare).
const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";

/// Board logical key in the keystore.
const BOARD_LOGICAL_KEY: &str = "board:default";

/// Result of bootstrap resolution.
pub struct BoardBootstrap {
    pub record_key: String,
    pub owner_secret: Option<String>,
    pub source: &'static str,
}

/// Try to resolve the bootstrap board key. Returns None if no external source found
/// (caller should create a new board or use existing local one).
pub async fn resolve_bootstrap() -> Option<BoardBootstrap> {
    // 1. Check env vars first.
    if let Ok(key) = std::env::var("HVOC_BOARD_KEY") {
        let secret = std::env::var("HVOC_BOARD_SECRET").ok();
        tracing::info!("Board bootstrap from env var: {}", &key[..20.min(key.len())]);
        return Some(BoardBootstrap {
            record_key: key,
            owner_secret: secret,
            source: "env",
        });
    }

    // 2. Query DNS TXT record.
    let domain = std::env::var("HVOC_BOOTSTRAP_DOMAIN")
        .unwrap_or_else(|_| DEFAULT_BOOTSTRAP_DOMAIN.to_string());

    match query_dns_txt(&domain).await {
        Ok(Some(bootstrap)) => {
            tracing::info!("Board bootstrap from DNS ({}): {}", domain,
                &bootstrap.record_key[..20.min(bootstrap.record_key.len())]);
            Some(bootstrap)
        }
        Ok(None) => {
            tracing::info!("No board bootstrap found in DNS TXT for {domain}");
            None
        }
        Err(e) => {
            tracing::warn!("DNS bootstrap query failed for {domain}: {e}");
            None
        }
    }
}

/// Query a DNS TXT record via DNS-over-HTTPS (Cloudflare).
/// Expects TXT record format: "hvoc-board=<record_key> hvoc-secret=<owner_keypair>"
async fn query_dns_txt(domain: &str) -> Result<Option<BoardBootstrap>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .get(DOH_URL)
        .header("Accept", "application/dns-json")
        .query(&[("name", domain), ("type", "TXT")])
        .send()
        .await
        .map_err(|e| format!("DNS query failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("DNS query returned status {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse DNS response: {e}"))?;

    // Parse Cloudflare DoH JSON response.
    // Format: { "Answer": [{ "type": 16, "data": "\"hvoc-board=... hvoc-secret=...\"" }] }
    let answers = match body.get("Answer").and_then(|a| a.as_array()) {
        Some(a) => a,
        None => return Ok(None),
    };

    for answer in answers {
        // TXT records have type 16.
        let record_type = answer.get("type").and_then(|t| t.as_u64()).unwrap_or(0);
        if record_type != 16 {
            continue;
        }

        let data = match answer.get("data").and_then(|d| d.as_str()) {
            Some(d) => d.trim_matches('"'),
            None => continue,
        };

        // DNS providers may insert whitespace into long TXT values.
        // Remove all whitespace first, then split on known keys.
        let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();

        let mut board_key = None;
        let mut board_secret = None;

        // Find hvoc-board= and hvoc-secret= in the compacted string.
        if let Some(start) = compact.find("hvoc-board=") {
            let val_start = start + "hvoc-board=".len();
            // Value runs until next known key or end of string.
            let val_end = compact[val_start..].find("hvoc-secret=")
                .map(|i| val_start + i)
                .unwrap_or(compact.len());
            board_key = Some(compact[val_start..val_end].to_string());
        }
        if let Some(start) = compact.find("hvoc-secret=") {
            let val_start = start + "hvoc-secret=".len();
            let val_end = compact[val_start..].find("hvoc-board=")
                .map(|i| val_start + i)
                .unwrap_or(compact.len());
            board_secret = Some(compact[val_start..val_end].to_string());
        }

        if let Some(key) = board_key {
            return Ok(Some(BoardBootstrap {
                record_key: key,
                owner_secret: board_secret,
                source: "dns",
            }));
        }
    }

    Ok(None)
}

/// Ensure the board index exists: bootstrap from DNS/env, or create new.
/// Called once on startup before reconciliation.
pub async fn ensure_board_index(state: &Arc<AppState>) {
    let ks = hvoc_store::Keystore(&state.store);

    // Check if we already have a board key locally.
    let existing = ks.get_dht_key(BOARD_LOGICAL_KEY).await.ok().flatten();

    // Try external bootstrap sources.
    if let Some(bootstrap) = resolve_bootstrap().await {
        // Save the bootstrap board key to local keystore (overwrite if different).
        let should_update = match &existing {
            Some((existing_key, _)) => existing_key != &bootstrap.record_key,
            None => true,
        };

        if should_update {
            let _ = ks.save_dht_key(
                BOARD_LOGICAL_KEY,
                &bootstrap.record_key,
                bootstrap.owner_secret.as_deref(),
            ).await;
            tracing::info!(
                "Board index set from {} bootstrap: {}",
                bootstrap.source,
                &bootstrap.record_key[..20.min(bootstrap.record_key.len())]
            );
        }
        return;
    }

    // No external bootstrap — use existing local key or create new.
    if existing.is_some() {
        tracing::info!("Using existing local board index");
        return;
    }

    // Create a brand new board index DHT record.
    let rc = match state.node.routing_context() {
        Ok(rc) => rc,
        Err(e) => {
            tracing::warn!("Cannot create board index — no routing context: {e}");
            return;
        }
    };

    let schema = veilid_core::DHTSchema::dflt(1).unwrap();
    match hvoc_veilid::dht::create_record(&rc, schema, None).await {
        Ok((record_key, owner_kp)) => {
            let secret = owner_kp.map(|kp| kp.to_string());
            let rk_str = record_key.to_string();
            let _ = ks.save_dht_key(BOARD_LOGICAL_KEY, &rk_str, secret.as_deref()).await;
            let _ = hvoc_veilid::dht::close_record(&rc, record_key).await;

            tracing::info!("Created new board index: {rk_str}");
            tracing::info!("To share this board, set a DNS TXT record:");
            tracing::info!("  Domain: _hvoc-board.hvck.academy");
            if let Some(ref s) = secret {
                tracing::info!("  Value:  \"hvoc-board={rk_str} hvoc-secret={s}\"");
            } else {
                tracing::info!("  Value:  \"hvoc-board={rk_str}\"");
            }
            tracing::info!("Or set env vars: HVOC_BOARD_KEY={rk_str}");
        }
        Err(e) => {
            tracing::warn!("Failed to create board index: {e}");
        }
    }
}

/// API: Get current board info (for sharing).
pub async fn get_board_info(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let ks = hvoc_store::Keystore(&state.store);
    match ks.get_dht_key(BOARD_LOGICAL_KEY).await {
        Ok(Some((record_key, owner_secret))) => {
            let has_write = owner_secret.is_some();
            let mut info = serde_json::json!({
                "board_key": record_key,
                "writable": has_write,
            });

            // Build shareable DNS TXT value.
            let mut txt = format!("hvoc-board={record_key}");
            if let Some(ref s) = owner_secret {
                txt.push_str(&format!(" hvoc-secret={s}"));
            }
            info["dns_txt_value"] = serde_json::json!(txt);

            // Build env var string for sharing.
            info["env_vars"] = serde_json::json!({
                "HVOC_BOARD_KEY": record_key,
                "HVOC_BOARD_SECRET": owner_secret,
            });

            Json(info)
        }
        Ok(None) => Json(serde_json::json!({ "error": "no board index configured" })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
