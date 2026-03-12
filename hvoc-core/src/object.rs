//! Network object types.
//!
//! Every object is:
//!   - canonical-serialisable (JSON, keys sorted)
//!   - content-addressed (object_id = BLAKE3 of canonical signable bytes)
//!   - signed by its author (signature produced externally by hvoc-veilid)
//!
//! These types carry raw signature bytes. Signing and verification are performed
//! by the crypto layer in hvoc-veilid using Veilid's CryptoSystem.

use crate::{canon, AuthorId, CoreError, ObjectId};
use serde::{Deserialize, Serialize};

// ─── Object kinds ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Thread,
    Post,
    DirectMessage,
    Profile,
    Tombstone,
}

// ─── Signable body helpers ────────────────────────────────────────────────────
// Each object type has a corresponding Signable* struct that contains only the
// fields included in the signature. This is serialised canonically, hashed for
// the object_id, and signed by the author's key.

#[derive(Serialize)]
struct SignableThread<'a> {
    kind: &'a str,
    author_id: &'a str,
    title: &'a str,
    created_at: i64,
    tags: &'a [String],
}

#[derive(Serialize)]
struct SignablePost<'a> {
    kind: &'a str,
    author_id: &'a str,
    thread_id: &'a str,
    parent_id: Option<&'a str>,
    body: &'a str,
    created_at: i64,
}

#[derive(Serialize)]
struct SignableProfile<'a> {
    kind: &'a str,
    author_id: &'a str,
    handle: &'a str,
    bio: &'a str,
    created_at: i64,
}

#[derive(Serialize)]
struct SignableTombstone<'a> {
    kind: &'a str,
    author_id: &'a str,
    target_id: &'a str,
    reason: Option<&'a str>,
    created_at: i64,
}

// ─── Thread ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub object_id: ObjectId,
    pub kind: ObjectKind,
    pub author_id: AuthorId,
    pub title: String,
    pub created_at: i64,
    pub tags: Vec<String>,
    /// Raw signature bytes (produced by Veilid CryptoSystem).
    #[serde(with = "hex_bytes")]
    pub signature: Vec<u8>,
}

impl Thread {
    /// Canonical bytes of the signable body (for signing/verification).
    pub fn signable_bytes(
        author_id: &str,
        title: &str,
        created_at: i64,
        tags: &[String],
    ) -> Result<Vec<u8>, CoreError> {
        canon::canonical_bytes(&SignableThread {
            kind: "thread",
            author_id,
            title,
            created_at,
            tags,
        })
    }

    /// Content ID from signable fields.
    pub fn compute_id(
        author_id: &str,
        title: &str,
        created_at: i64,
        tags: &[String],
    ) -> Result<ObjectId, CoreError> {
        canon::content_id(&SignableThread {
            kind: "thread",
            author_id,
            title,
            created_at,
            tags,
        })
    }

    /// Construct from pre-computed values (after signing in hvoc-veilid).
    pub fn new(
        author_id: AuthorId,
        title: String,
        created_at: i64,
        tags: Vec<String>,
        object_id: ObjectId,
        signature: Vec<u8>,
    ) -> Self {
        Thread {
            object_id,
            kind: ObjectKind::Thread,
            author_id,
            title,
            created_at,
            tags,
            signature,
        }
    }

    /// Re-derive signable bytes from this thread's fields.
    pub fn to_signable_bytes(&self) -> Result<Vec<u8>, CoreError> {
        Self::signable_bytes(&self.author_id, &self.title, self.created_at, &self.tags)
    }

    /// Verify that the object_id matches the content.
    pub fn verify_id(&self) -> Result<(), CoreError> {
        let expected = Self::compute_id(&self.author_id, &self.title, self.created_at, &self.tags)?;
        if expected != self.object_id {
            return Err(CoreError::IdMismatch {
                expected,
                actual: self.object_id.clone(),
            });
        }
        Ok(())
    }
}

// ─── Post ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub object_id: ObjectId,
    pub kind: ObjectKind,
    pub author_id: AuthorId,
    pub thread_id: ObjectId,
    pub parent_id: Option<ObjectId>,
    pub body: String,
    pub created_at: i64,
    #[serde(with = "hex_bytes")]
    pub signature: Vec<u8>,
}

impl Post {
    pub fn signable_bytes(
        author_id: &str,
        thread_id: &str,
        parent_id: Option<&str>,
        body: &str,
        created_at: i64,
    ) -> Result<Vec<u8>, CoreError> {
        canon::canonical_bytes(&SignablePost {
            kind: "post",
            author_id,
            thread_id,
            parent_id,
            body,
            created_at,
        })
    }

    pub fn compute_id(
        author_id: &str,
        thread_id: &str,
        parent_id: Option<&str>,
        body: &str,
        created_at: i64,
    ) -> Result<ObjectId, CoreError> {
        canon::content_id(&SignablePost {
            kind: "post",
            author_id,
            thread_id,
            parent_id,
            body,
            created_at,
        })
    }

    pub fn new(
        author_id: AuthorId,
        thread_id: ObjectId,
        parent_id: Option<ObjectId>,
        body: String,
        created_at: i64,
        object_id: ObjectId,
        signature: Vec<u8>,
    ) -> Self {
        Post {
            object_id,
            kind: ObjectKind::Post,
            author_id,
            thread_id,
            parent_id,
            body,
            created_at,
            signature,
        }
    }

    pub fn to_signable_bytes(&self) -> Result<Vec<u8>, CoreError> {
        Self::signable_bytes(
            &self.author_id,
            &self.thread_id,
            self.parent_id.as_deref(),
            &self.body,
            self.created_at,
        )
    }

    pub fn verify_id(&self) -> Result<(), CoreError> {
        let expected = Self::compute_id(
            &self.author_id,
            &self.thread_id,
            self.parent_id.as_deref(),
            &self.body,
            self.created_at,
        )?;
        if expected != self.object_id {
            return Err(CoreError::IdMismatch {
                expected,
                actual: self.object_id.clone(),
            });
        }
        Ok(())
    }
}

// ─── Profile ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub object_id: ObjectId,
    pub kind: ObjectKind,
    pub author_id: AuthorId,
    pub handle: String,
    pub bio: String,
    pub created_at: i64,
    #[serde(with = "hex_bytes")]
    pub signature: Vec<u8>,
}

impl Profile {
    pub fn signable_bytes(
        author_id: &str,
        handle: &str,
        bio: &str,
        created_at: i64,
    ) -> Result<Vec<u8>, CoreError> {
        canon::canonical_bytes(&SignableProfile {
            kind: "profile",
            author_id,
            handle,
            bio,
            created_at,
        })
    }

    pub fn compute_id(
        author_id: &str,
        handle: &str,
        bio: &str,
        created_at: i64,
    ) -> Result<ObjectId, CoreError> {
        canon::content_id(&SignableProfile {
            kind: "profile",
            author_id,
            handle,
            bio,
            created_at,
        })
    }

    pub fn new(
        author_id: AuthorId,
        handle: String,
        bio: String,
        created_at: i64,
        object_id: ObjectId,
        signature: Vec<u8>,
    ) -> Self {
        Profile {
            object_id,
            kind: ObjectKind::Profile,
            author_id,
            handle,
            bio,
            created_at,
            signature,
        }
    }

    pub fn to_signable_bytes(&self) -> Result<Vec<u8>, CoreError> {
        Self::signable_bytes(&self.author_id, &self.handle, &self.bio, self.created_at)
    }
}

// ─── Tombstone ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tombstone {
    pub object_id: ObjectId,
    pub kind: ObjectKind,
    pub author_id: AuthorId,
    pub target_id: ObjectId,
    pub reason: Option<String>,
    pub created_at: i64,
    #[serde(with = "hex_bytes")]
    pub signature: Vec<u8>,
}

impl Tombstone {
    pub fn signable_bytes(
        author_id: &str,
        target_id: &str,
        reason: Option<&str>,
        created_at: i64,
    ) -> Result<Vec<u8>, CoreError> {
        canon::canonical_bytes(&SignableTombstone {
            kind: "tombstone",
            author_id,
            target_id,
            reason,
            created_at,
        })
    }

    pub fn compute_id(
        author_id: &str,
        target_id: &str,
        reason: Option<&str>,
        created_at: i64,
    ) -> Result<ObjectId, CoreError> {
        canon::content_id(&SignableTombstone {
            kind: "tombstone",
            author_id,
            target_id,
            reason,
            created_at,
        })
    }

    pub fn new(
        author_id: AuthorId,
        target_id: ObjectId,
        reason: Option<String>,
        created_at: i64,
        object_id: ObjectId,
        signature: Vec<u8>,
    ) -> Self {
        Tombstone {
            object_id,
            kind: ObjectKind::Tombstone,
            author_id,
            target_id,
            reason,
            created_at,
            signature,
        }
    }

    pub fn to_signable_bytes(&self) -> Result<Vec<u8>, CoreError> {
        Self::signable_bytes(
            &self.author_id,
            &self.target_id,
            self.reason.as_deref(),
            self.created_at,
        )
    }
}

// ─── DirectMessage envelope ──────────────────────────────────────────────────

/// Encrypted DM envelope. Plaintext is never stored in this struct.
///
/// Encryption: sender ephemeral X25519 → ECDH → generate_shared_secret with
/// domain "hvoc-dm-v1" → XChaCha20-Poly1305 AEAD.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessage {
    pub object_id: ObjectId,
    pub kind: ObjectKind,
    pub sender_id: AuthorId,
    pub recipient_id: AuthorId,
    /// Hex-encoded ephemeral X25519 public key.
    pub ephemeral_pubkey: String,
    /// Hex-encoded nonce.
    pub nonce: String,
    /// Base64-encoded ciphertext.
    pub ciphertext: String,
    pub sent_at: i64,
    #[serde(with = "hex_bytes")]
    pub signature: Vec<u8>,
}

/// Plaintext DM payload (serialised before encryption).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmPayload {
    pub body: String,
    pub sent_at: i64,
}

// ─── HvocObject sum type ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HvocObject {
    Thread(Thread),
    Post(Post),
    Profile(Profile),
    DirectMessage(DirectMessage),
    Tombstone(Tombstone),
}

impl HvocObject {
    pub fn object_id(&self) -> &str {
        match self {
            HvocObject::Thread(t) => &t.object_id,
            HvocObject::Post(p) => &p.object_id,
            HvocObject::Profile(p) => &p.object_id,
            HvocObject::DirectMessage(d) => &d.object_id,
            HvocObject::Tombstone(t) => &t.object_id,
        }
    }

    pub fn author_id(&self) -> &str {
        match self {
            HvocObject::Thread(t) => &t.author_id,
            HvocObject::Post(p) => &p.author_id,
            HvocObject::Profile(p) => &p.author_id,
            HvocObject::DirectMessage(d) => &d.sender_id,
            HvocObject::Tombstone(t) => &t.author_id,
        }
    }
}

// ─── Hex bytes serde helper ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_id_is_deterministic() {
        let id1 = Thread::compute_id("author1", "Test Thread", 1000, &[]).unwrap();
        let id2 = Thread::compute_id("author1", "Test Thread", 1000, &[]).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64);
    }

    #[test]
    fn thread_id_changes_with_input() {
        let id1 = Thread::compute_id("author1", "Thread A", 1000, &[]).unwrap();
        let id2 = Thread::compute_id("author1", "Thread B", 1000, &[]).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn thread_verify_id_passes() {
        let thread = Thread::new(
            "author1".to_string(),
            "Test".to_string(),
            1000,
            vec![],
            Thread::compute_id("author1", "Test", 1000, &[]).unwrap(),
            vec![0u8; 64],
        );
        assert!(thread.verify_id().is_ok());
    }

    #[test]
    fn thread_verify_id_fails_on_mismatch() {
        let thread = Thread::new(
            "author1".to_string(),
            "Test".to_string(),
            1000,
            vec![],
            "wrong_id".to_string(),
            vec![0u8; 64],
        );
        assert!(thread.verify_id().is_err());
    }

    #[test]
    fn post_id_is_deterministic() {
        let id1 = Post::compute_id("author1", "thread1", None, "hello", 2000).unwrap();
        let id2 = Post::compute_id("author1", "thread1", None, "hello", 2000).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn post_with_parent_id_differs() {
        let id1 = Post::compute_id("a", "t", None, "body", 1).unwrap();
        let id2 = Post::compute_id("a", "t", Some("parent"), "body", 1).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn profile_id_is_deterministic() {
        let id1 = Profile::compute_id("author1", "handle1", "bio", 3000).unwrap();
        let id2 = Profile::compute_id("author1", "handle1", "bio", 3000).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn tombstone_id_is_deterministic() {
        let id1 = Tombstone::compute_id("author1", "target1", Some("reason"), 4000).unwrap();
        let id2 = Tombstone::compute_id("author1", "target1", Some("reason"), 4000).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn tombstone_none_reason_differs() {
        let id1 = Tombstone::compute_id("a", "t", None, 1).unwrap();
        let id2 = Tombstone::compute_id("a", "t", Some("r"), 1).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn signable_bytes_are_stable() {
        let b1 = Thread::signable_bytes("a", "title", 100, &["tag".to_string()]).unwrap();
        let b2 = Thread::signable_bytes("a", "title", 100, &["tag".to_string()]).unwrap();
        assert_eq!(b1, b2);
        assert!(!b1.is_empty());
    }

    #[test]
    fn hvoc_object_sum_type() {
        let thread = Thread::new(
            "a".into(), "t".into(), 1, vec![],
            Thread::compute_id("a", "t", 1, &[]).unwrap(),
            vec![],
        );
        let obj = HvocObject::Thread(thread);
        assert_eq!(obj.author_id(), "a");
        assert!(!obj.object_id().is_empty());
    }

    #[test]
    fn dm_payload_serde_roundtrip() {
        let payload = DmPayload { body: "secret message".into(), sent_at: 12345 };
        let json = serde_json::to_string(&payload).unwrap();
        let back: DmPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.body, "secret message");
        assert_eq!(back.sent_at, 12345);
    }
}

mod hex_bytes {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}
