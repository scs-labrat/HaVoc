pub mod keystore;
pub mod migrations;
pub mod repos;

pub use keystore::Keystore;
pub use repos::{BoardRepo, ContactRepo, IdentityRepo, MessageRepo, PostRepo, ThreadRepo, TombstoneRepo};

use std::path::Path;
use std::sync::Arc;
use rusqlite::Connection;
use tokio::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("keystore error: {0}")]
    Keystore(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("task error: {0}")]
    Task(String),
}

/// Local SQLite store — materialised view of DHT state.
/// Wrapped in Arc<Mutex> for async access from tokio tasks.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    /// Open (or create) the database at the given path and run migrations.
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        let path = path.to_path_buf();
        let conn = tokio::task::spawn_blocking(move || -> Result<Connection, StoreError> {
            let conn = Connection::open(&path)?;
            conn.execute_batch(migrations::SCHEMA)?;
            Ok(conn)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))??;

        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Get a reference to the inner connection (for repo operations).
    pub(crate) fn conn(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }
}
