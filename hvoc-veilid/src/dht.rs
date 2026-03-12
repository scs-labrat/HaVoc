//! DHT operations for HVOC.
//!
//! Maps HVOC's logical entities to Veilid DHT records:
//!   - Profile  → DFLT(1), subkey 0 = profile JSON
//!   - Thread   → DFLT(2), subkey 0 = header JSON, subkey 1 = post ID index
//!   - Post     → DFLT(1), subkey 0 = post body JSON
//!   - Inbox    → DFLT(1), subkey 0 = route blob for DM delivery
//!
//! Each entity gets its own DHT record. The RecordKey is stored locally in
//! hvoc-store's dht_keys table, mapped from a logical path.

use veilid_core::{
    DHTSchema, KeyPair, RecordKey, RoutingContext, ValueSubkey,
    CRYPTO_KIND_VLD0,
};

use crate::VeilidError;

/// Create a new DHT record with the given schema.
/// If `owner` is Some, the record is deterministic (same keypair + schema = same key).
pub async fn create_record(
    rc: &RoutingContext,
    schema: DHTSchema,
    owner: Option<KeyPair>,
) -> Result<(RecordKey, Option<KeyPair>), VeilidError> {
    let desc = rc
        .create_dht_record(CRYPTO_KIND_VLD0, schema, owner)
        .await?;
    let owner_kp = desc.owner_keypair();
    Ok((desc.key().clone(), owner_kp))
}

/// Open an existing DHT record for reading (no writer key).
pub async fn open_record_readonly(
    rc: &RoutingContext,
    key: RecordKey,
) -> Result<(), VeilidError> {
    let _ = rc.open_dht_record(key, None).await?;
    Ok(())
}

/// Open an existing DHT record for writing with the given writer keypair.
pub async fn open_record_writable(
    rc: &RoutingContext,
    key: RecordKey,
    writer: KeyPair,
) -> Result<(), VeilidError> {
    let _ = rc.open_dht_record(key, Some(writer)).await?;
    Ok(())
}

/// Close a DHT record.
pub async fn close_record(
    rc: &RoutingContext,
    key: RecordKey,
) -> Result<(), VeilidError> {
    rc.close_dht_record(key).await?;
    Ok(())
}

/// Delete a DHT record (must be closed first).
pub async fn delete_record(
    rc: &RoutingContext,
    key: RecordKey,
) -> Result<(), VeilidError> {
    rc.delete_dht_record(key).await?;
    Ok(())
}

/// Read a subkey value from a DHT record.
/// Returns None if the subkey has never been set.
pub async fn get_value(
    rc: &RoutingContext,
    key: RecordKey,
    subkey: ValueSubkey,
    force_refresh: bool,
) -> Result<Option<Vec<u8>>, VeilidError> {
    let val = rc.get_dht_value(key, subkey, force_refresh).await?;
    Ok(val.map(|v| v.data().to_vec()))
}

/// Write data to a DHT record subkey.
/// Returns Ok(None) on success, Ok(Some(newer_data)) if a conflict occurred.
pub async fn set_value(
    rc: &RoutingContext,
    key: RecordKey,
    subkey: ValueSubkey,
    data: Vec<u8>,
) -> Result<Option<Vec<u8>>, VeilidError> {
    let result = rc.set_dht_value(key, subkey, data, None).await?;
    Ok(result.map(|v| v.data().to_vec()))
}

/// Watch a DHT record for changes. Returns true if the watch is active.
pub async fn watch_record(
    rc: &RoutingContext,
    key: RecordKey,
) -> Result<bool, VeilidError> {
    let active = rc
        .watch_dht_values(key, None, None, None)
        .await?;
    Ok(active)
}

/// Cancel a watch on a DHT record.
pub async fn cancel_watch(
    rc: &RoutingContext,
    key: RecordKey,
) -> Result<(), VeilidError> {
    rc.cancel_dht_watch(key, None).await?;
    Ok(())
}

// ─── High-level entity operations ────────────────────────────────────────────

/// Publish a profile to DHT. Creates or updates the profile record.
pub async fn publish_profile(
    rc: &RoutingContext,
    record_key: RecordKey,
    profile_json: &[u8],
) -> Result<(), VeilidError> {
    let conflict = set_value(rc, record_key, 0, profile_json.to_vec()).await?;
    if conflict.is_some() {
        return Err(VeilidError::Dht("profile write conflict".into()));
    }
    Ok(())
}

/// Publish a thread header to subkey 0 of its DHT record.
pub async fn publish_thread_header(
    rc: &RoutingContext,
    record_key: RecordKey,
    thread_json: &[u8],
) -> Result<(), VeilidError> {
    let conflict = set_value(rc, record_key, 0, thread_json.to_vec()).await?;
    if conflict.is_some() {
        return Err(VeilidError::Dht("thread header write conflict".into()));
    }
    Ok(())
}

/// Update the thread index (subkey 1) with the current list of post IDs.
pub async fn update_thread_index(
    rc: &RoutingContext,
    record_key: RecordKey,
    post_ids_json: &[u8],
) -> Result<(), VeilidError> {
    let conflict = set_value(rc, record_key, 1, post_ids_json.to_vec()).await?;
    if conflict.is_some() {
        return Err(VeilidError::Dht("thread index write conflict".into()));
    }
    Ok(())
}

/// Publish a post body to subkey 0 of its DHT record.
pub async fn publish_post(
    rc: &RoutingContext,
    record_key: RecordKey,
    post_json: &[u8],
) -> Result<(), VeilidError> {
    let conflict = set_value(rc, record_key, 0, post_json.to_vec()).await?;
    if conflict.is_some() {
        return Err(VeilidError::Dht("post write conflict".into()));
    }
    Ok(())
}

/// Publish inbox route blob to subkey 0 of the inbox DHT record.
pub async fn publish_inbox(
    rc: &RoutingContext,
    record_key: RecordKey,
    route_blob: &[u8],
) -> Result<(), VeilidError> {
    let conflict = set_value(rc, record_key, 0, route_blob.to_vec()).await?;
    if conflict.is_some() {
        return Err(VeilidError::Dht("inbox write conflict".into()));
    }
    Ok(())
}

/// Fetch a profile from DHT by its record key.
pub async fn fetch_profile(
    rc: &RoutingContext,
    record_key: RecordKey,
) -> Result<Option<Vec<u8>>, VeilidError> {
    get_value(rc, record_key, 0, true).await
}

/// Fetch a thread header from DHT.
pub async fn fetch_thread_header(
    rc: &RoutingContext,
    record_key: RecordKey,
) -> Result<Option<Vec<u8>>, VeilidError> {
    get_value(rc, record_key, 0, true).await
}

/// Fetch the thread index (list of post IDs) from DHT.
pub async fn fetch_thread_index(
    rc: &RoutingContext,
    record_key: RecordKey,
) -> Result<Option<Vec<u8>>, VeilidError> {
    get_value(rc, record_key, 1, true).await
}

/// Fetch a post body from DHT.
pub async fn fetch_post(
    rc: &RoutingContext,
    record_key: RecordKey,
) -> Result<Option<Vec<u8>>, VeilidError> {
    get_value(rc, record_key, 0, true).await
}
