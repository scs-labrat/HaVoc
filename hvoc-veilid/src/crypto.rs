//! Crypto operations using Veilid's CryptoSystem.
//!
//! Wraps Veilid's VLD0 suite (Ed25519 + X25519 + XChaCha20-Poly1305 + BLAKE3)
//! to provide signing, verification, and DM encryption for HVOC objects.

use veilid_core::{
    CryptoSystem, KeyPair, PublicKey, SecretKey, Signature, Nonce,
    CRYPTO_KIND_VLD0,
};

use crate::VeilidError;
use hvoc_core::{AuthorId, DmPayload, Post, Profile, Thread, Tombstone};

/// Generate a new Ed25519 keypair via Veilid's crypto system.
pub fn generate_keypair(cs: &(dyn CryptoSystem + Send + Sync)) -> KeyPair {
    cs.generate_keypair()
}

/// Get the author_id string from a public key.
pub fn author_id_from_key(key: &PublicKey) -> AuthorId {
    key.to_string()
}

/// Sign arbitrary bytes and return the raw signature bytes.
pub fn sign(
    cs: &(dyn CryptoSystem + Send + Sync),
    key: &PublicKey,
    secret: &SecretKey,
    data: &[u8],
) -> Result<Vec<u8>, VeilidError> {
    let sig = cs
        .sign(key, secret, data)
        .map_err(|e| VeilidError::Crypto(e.to_string()))?;
    Ok(sig.value().bytes().to_vec())
}

/// Verify a signature against data and a public key.
pub fn verify(
    cs: &(dyn CryptoSystem + Send + Sync),
    key: &PublicKey,
    data: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, VeilidError> {
    use std::convert::TryFrom;
    let bare = veilid_core::BareSignature::try_from(signature_bytes)
        .map_err(|e| VeilidError::Crypto(format!("invalid signature bytes: {e}")))?;
    let sig = Signature::new(CRYPTO_KIND_VLD0, bare);
    cs.verify(key, data, &sig)
        .map_err(|e| VeilidError::Crypto(e.to_string()))
}

// ─── Object creation helpers ─────────────────────────────────────────────────

/// Create a signed Thread.
pub fn create_thread(
    cs: &(dyn CryptoSystem + Send + Sync),
    keypair: &KeyPair,
    title: &str,
    tags: Vec<String>,
) -> Result<Thread, VeilidError> {
    let pub_key = keypair.key();
    let secret = keypair.secret();
    let author_id = author_id_from_key(&pub_key);
    let now = chrono::Utc::now().timestamp();

    let bytes = Thread::signable_bytes(&author_id, title, now, &tags)?;
    let object_id = Thread::compute_id(&author_id, title, now, &tags)?;
    let sig = sign(cs, &pub_key, &secret, &bytes)?;

    Ok(Thread::new(
        author_id,
        title.to_string(),
        now,
        tags,
        object_id,
        sig,
    ))
}

/// Create a signed Post.
pub fn create_post(
    cs: &(dyn CryptoSystem + Send + Sync),
    keypair: &KeyPair,
    thread_id: &str,
    parent_id: Option<&str>,
    body: &str,
) -> Result<Post, VeilidError> {
    let pub_key = keypair.key();
    let secret = keypair.secret();
    let author_id = author_id_from_key(&pub_key);
    let now = chrono::Utc::now().timestamp();

    let bytes = Post::signable_bytes(&author_id, thread_id, parent_id, body, now)?;
    let object_id = Post::compute_id(&author_id, thread_id, parent_id, body, now)?;
    let sig = sign(cs, &pub_key, &secret, &bytes)?;

    Ok(Post::new(
        author_id,
        thread_id.to_string(),
        parent_id.map(|s| s.to_string()),
        body.to_string(),
        now,
        object_id,
        sig,
    ))
}

/// Create a signed Profile.
pub fn create_profile(
    cs: &(dyn CryptoSystem + Send + Sync),
    keypair: &KeyPair,
    handle: &str,
    bio: &str,
) -> Result<Profile, VeilidError> {
    let pub_key = keypair.key();
    let secret = keypair.secret();
    let author_id = author_id_from_key(&pub_key);
    let now = chrono::Utc::now().timestamp();

    let bytes = Profile::signable_bytes(&author_id, handle, bio, now)?;
    let object_id = Profile::compute_id(&author_id, handle, bio, now)?;
    let sig = sign(cs, &pub_key, &secret, &bytes)?;

    Ok(Profile::new(
        author_id,
        handle.to_string(),
        bio.to_string(),
        now,
        object_id,
        sig,
    ))
}

/// Create a signed Tombstone (soft-delete marker for a post or thread).
pub fn create_tombstone(
    cs: &(dyn CryptoSystem + Send + Sync),
    keypair: &KeyPair,
    target_id: &str,
    reason: Option<&str>,
) -> Result<Tombstone, VeilidError> {
    let pub_key = keypair.key();
    let secret = keypair.secret();
    let author_id = author_id_from_key(&pub_key);
    let now = chrono::Utc::now().timestamp();

    let bytes = Tombstone::signable_bytes(&author_id, target_id, reason, now)?;
    let object_id = Tombstone::compute_id(&author_id, target_id, reason, now)?;
    let sig = sign(cs, &pub_key, &secret, &bytes)?;

    Ok(Tombstone::new(
        author_id,
        target_id.to_string(),
        reason.map(|s| s.to_string()),
        now,
        object_id,
        sig,
    ))
}

/// Verify any object's signature.
pub fn verify_object(
    cs: &(dyn CryptoSystem + Send + Sync),
    author_key: &PublicKey,
    signable_bytes: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, VeilidError> {
    verify(cs, author_key, signable_bytes, signature_bytes)
}

// ─── DM encryption ──────────────────────────────────────────────────────────

/// Encrypted DM envelope for wire transport.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncryptedDm {
    pub sender_id: String,
    pub recipient_id: String,
    pub nonce: String,
    pub ciphertext: String,
    pub sent_at: i64,
}

/// Encrypt a DM payload using ECDH + XChaCha20-Poly1305.
///
/// Uses sender's secret + recipient's public key → shared secret → AEAD encrypt.
pub fn encrypt_dm(
    cs: &(dyn CryptoSystem + Send + Sync),
    sender_keypair: &KeyPair,
    recipient_pub: &PublicKey,
    body: &str,
) -> Result<EncryptedDm, VeilidError> {
    let now = chrono::Utc::now().timestamp();
    let payload = DmPayload {
        body: body.to_string(),
        sent_at: now,
    };
    let plaintext = serde_json::to_vec(&payload)
        .map_err(|e| VeilidError::Crypto(format!("serialize DM payload: {e}")))?;

    // ECDH: sender secret + recipient public → shared secret with domain separation.
    let shared = cs
        .generate_shared_secret(recipient_pub, &sender_keypair.secret(), b"hvoc-dm-v1")
        .map_err(|e| VeilidError::Crypto(e.to_string()))?;

    // Random nonce.
    let nonce = cs.random_nonce();

    // AEAD encrypt.
    let ciphertext = cs
        .encrypt_aead(&plaintext, &nonce, &shared, None)
        .map_err(|e| VeilidError::Crypto(e.to_string()))?;

    let sender_id = author_id_from_key(&sender_keypair.key());
    let recipient_id = author_id_from_key(recipient_pub);

    Ok(EncryptedDm {
        sender_id,
        recipient_id,
        nonce: hex::encode(nonce.as_ref()),
        ciphertext: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &ciphertext,
        ),
        sent_at: now,
    })
}

/// Decrypt a DM envelope using ECDH + XChaCha20-Poly1305.
///
/// Uses recipient's secret + sender's public key → shared secret → AEAD decrypt.
pub fn decrypt_dm(
    cs: &(dyn CryptoSystem + Send + Sync),
    recipient_keypair: &KeyPair,
    sender_pub: &PublicKey,
    envelope: &EncryptedDm,
) -> Result<DmPayload, VeilidError> {
    // Reconstruct shared secret (ECDH is commutative).
    let shared = cs
        .generate_shared_secret(sender_pub, &recipient_keypair.secret(), b"hvoc-dm-v1")
        .map_err(|e| VeilidError::Crypto(e.to_string()))?;

    let nonce_bytes = hex::decode(&envelope.nonce)
        .map_err(|e| VeilidError::Crypto(format!("invalid nonce hex: {e}")))?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice())
        .map_err(|e| VeilidError::Crypto(format!("invalid nonce: {e}")))?;

    let ciphertext = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &envelope.ciphertext,
    )
    .map_err(|e| VeilidError::Crypto(format!("invalid ciphertext base64: {e}")))?;

    let plaintext = cs
        .decrypt_aead(&ciphertext, &nonce, &shared, None)
        .map_err(|e| VeilidError::Crypto(format!("AEAD decrypt failed: {e}")))?;

    let payload: DmPayload = serde_json::from_slice(&plaintext)
        .map_err(|e| VeilidError::Crypto(format!("invalid DM payload JSON: {e}")))?;

    Ok(payload)
}
