//! Encrypted identity persistence using rusqlite.

use crate::{Store, StoreError};
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct StoredIdentity {
    pub id: String,
    pub handle: String,
    pub bio: String,
    pub created_at: i64,
}

pub struct Keystore<'a>(pub &'a Store);

impl<'a> Keystore<'a> {
    pub async fn save(
        &self,
        id: &str,
        handle: &str,
        secret_bytes: &[u8],
        passphrase: &[u8],
    ) -> Result<(), StoreError> {
        let salt = blake3::hash(&[passphrase, id.as_bytes()].concat()).as_bytes()[..16].to_vec();
        let key = blake3::derive_key("hvoc-keystore-v1", &[passphrase, &salt].concat());

        let encrypted: Vec<u8> = secret_bytes
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % 32])
            .collect();

        let encrypted_b64 = b64_encode(&encrypted);
        let salt_hex = hex::encode(&salt);
        let now = Utc::now().timestamp();

        let id = id.to_string();
        let handle = handle.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO keystore (id, handle, bio, created_at, encrypted_seed, kdf_salt)
                 VALUES (?1, ?2, '', ?3, ?4, ?5)",
                rusqlite::params![id, handle, now, encrypted_b64, salt_hex],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn load(&self, id: &str, passphrase: &[u8]) -> Result<Vec<u8>, StoreError> {
        let id = id.to_string();
        let conn = self.0.conn().clone();

        let (encrypted_b64, salt_hex) = tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT encrypted_seed, kdf_salt FROM keystore WHERE id = ?1",
            )?;
            stmt.query_row(rusqlite::params![id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|_| StoreError::NotFound(format!("identity {}", id)))
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))??;

        let encrypted =
            b64_decode(&encrypted_b64).map_err(|e| StoreError::Keystore(e.to_string()))?;
        let salt = hex::decode(&salt_hex).map_err(|e| StoreError::Keystore(e.to_string()))?;

        let key = blake3::derive_key("hvoc-keystore-v1", &[passphrase, &salt].concat());
        let decrypted: Vec<u8> = encrypted
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % 32])
            .collect();

        Ok(decrypted)
    }

    pub async fn list_ids(&self) -> Result<Vec<StoredIdentity>, StoreError> {
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT id, handle, bio, created_at FROM keystore ORDER BY created_at",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(StoredIdentity {
                        id: row.get(0)?,
                        handle: row.get(1)?,
                        bio: row.get::<_, String>(2).unwrap_or_default(),
                        created_at: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn save_dht_key(
        &self,
        logical_key: &str,
        record_key: &str,
        owner_secret: Option<&str>,
    ) -> Result<(), StoreError> {
        let logical_key = logical_key.to_string();
        let record_key = record_key.to_string();
        let owner_secret = owner_secret.map(|s| s.to_string());
        let now = Utc::now().timestamp();
        let is_owned = owner_secret.is_some() as i32;
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO dht_keys (logical_key, record_key, owner_secret, is_owned, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![logical_key, record_key, owner_secret, is_owned, now],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn get_dht_key(
        &self,
        logical_key: &str,
    ) -> Result<Option<(String, Option<String>)>, StoreError> {
        let logical_key = logical_key.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || -> Result<Option<(String, Option<String>)>, StoreError> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT record_key, owner_secret FROM dht_keys WHERE logical_key = ?1",
            )?;
            let result = stmt
                .query_row(rusqlite::params![logical_key], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                });
            match result {
                Ok(row) => Ok(Some(row)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(StoreError::Db(e)),
            }
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

fn b64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn b64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s)
}
