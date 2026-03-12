# hvoc — Veilid-native P2P Forum + Messaging

Rust workspace mapping the browser prototype to a real distributed system.

---

## Architecture

```
hvoc-core      Pure domain layer — types, signing, canonical serialisation
               No I/O. No tokio. Easy to unit-test.

hvoc-veilid    All Veilid I/O — DHT ops, routing contexts, watches, DM transport
               Depends on: hvoc-core, veilid-core, tokio

hvoc-store     Local SQLite materialised view + encrypted keystore
               Depends on: hvoc-core, rusqlite

hvoc-api       Local HTTP/WebSocket bridge for the UI
               Depends on: all three above, axum

hvoc-cli       CLI entrypoint + serve command
               Depends on: all four above, clap
```

## DHT Key Schema

| Logical key | Content | Schema |
|---|---|---|
| `profile:<author_id>` | Signed Profile JSON | DFLT(1), subkey 0 |
| `thread:<thread_id>` | Thread header + post index | DFLT(2), subkeys 0-1 |
| `post:<post_id>` | Signed Post body | DFLT(1), subkey 0 |
| `inbox:<author_id>` | Private route blob for DM delivery | DFLT(1), subkey 0 |
| `board:default` | Board index (array of thread entries) | DFLT(1), subkey 0 |

## Object Model

Every network object is:
1. Canonically serialised (JSON with keys sorted lexicographically)
2. Content-addressed: `object_id = BLAKE3(canonical_bytes_without_signature)`
3. Ed25519-signed by the author

Object kinds: `thread | post | direct_message | profile | tombstone`

## DM Encryption (v1)

```
sender identity keypair (Ed25519/X25519)
ECDH(sender_secret, recipient_pubkey) + domain "hvoc-dm-v1" → SharedSecret
XChaCha20-Poly1305(shared_secret, random_nonce, DmPayload JSON) → ciphertext
Envelope: { sender_id, recipient_id, nonce, ciphertext, sent_at }
Delivered via: Veilid AppMessage → recipient's private route (from inbox DHT record)
```

## Features

- **Forum**: Create threads with tags, post replies, thread discovery via shared board index
- **Encrypted DMs**: End-to-end encrypted messaging via Veilid's ECDH + AEAD
- **Private routes**: Inbox route published to DHT for inbound DM delivery
- **Identity**: Ed25519 keypairs, passphrase-encrypted local storage, profile publish to DHT
- **Tombstones**: Delete own posts/threads (soft-delete, propagated via DHT)
- **Sync**: Background DHT reconciliation on startup, WebSocket live updates
- **QR invites**: Generate/scan QR codes to share identity and add contacts
- **Handle resolution**: Display names instead of truncated public keys
- **Notifications**: Desktop notifications for incoming DMs

## Build

```bash
# Requirements: Rust 1.76+, cargo
cargo build

# Run the API server
cargo run -p hvoc-cli -- serve

# CLI commands
cargo run -p hvoc-cli -- identity list
cargo run -p hvoc-cli -- thread list
cargo run -p hvoc-cli -- post list --thread <id>

# Open hvoc.html in a browser to use the web UI (connects to localhost:7734)
```

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/identity` | Get active identity |
| POST | `/api/identity` | Create identity (handle + passphrase) |
| POST | `/api/identity/unlock` | Unlock existing identity |
| GET | `/api/identity/list` | List all local identities |
| GET | `/api/threads?limit=N&offset=N` | List threads (paginated) |
| POST | `/api/threads` | Create thread (title + body + tags) |
| GET | `/api/threads/{id}` | Get thread details |
| GET | `/api/threads/{id}/posts` | List posts in thread |
| POST | `/api/threads/{id}/posts` | Create post / reply |
| DELETE | `/api/threads/{id}` | Tombstone a thread (author only) |
| DELETE | `/api/posts/{id}` | Tombstone a post (author only) |
| GET | `/api/messages?peer_id=X` | List messages (optionally filtered by peer) |
| POST | `/api/messages` | Send encrypted DM |
| GET | `/api/contacts` | List contacts |
| POST | `/api/contacts` | Add contact (via invite) |
| GET | `/api/profiles/{author_id}` | Get profile |
| POST | `/api/profiles/resolve` | Batch resolve handles |
| GET | `/ws` | WebSocket for live sync events |

## Testing

```bash
cargo test                    # Unit tests
cargo test -p hvoc-core       # Core type tests
cargo test -p hvoc-store      # Store/repo tests
```
