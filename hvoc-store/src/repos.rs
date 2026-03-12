//! Repository types for threads and posts using rusqlite.

use crate::{Store, StoreError};
use serde::{Deserialize, Serialize};

// ─── Thread repository ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadRow {
    pub object_id: String,
    pub author_id: String,
    pub title: String,
    pub tags: String,
    pub created_at: i64,
    pub post_count: i64,
    pub last_post_at: Option<i64>,
}

pub struct ThreadRepo<'a>(pub &'a Store);

impl<'a> ThreadRepo<'a> {
    pub async fn insert(&self, thread: &hvoc_core::Thread) -> Result<(), StoreError> {
        let raw_json = serde_json::to_string(thread)?;
        let tags_json = serde_json::to_string(&thread.tags)?;
        let object_id = thread.object_id.clone();
        let author_id = thread.author_id.clone();
        let title = thread.title.clone();
        let created_at = thread.created_at;
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO threads (object_id, author_id, title, tags, created_at, post_count, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
                rusqlite::params![object_id, author_id, title, tags_json, created_at, raw_json],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn get(&self, object_id: &str) -> Result<ThreadRow, StoreError> {
        let object_id = object_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, author_id, title, tags, created_at, post_count, last_post_at
                 FROM threads WHERE object_id = ?1",
            )?;
            stmt.query_row(rusqlite::params![object_id], |row| {
                Ok(ThreadRow {
                    object_id: row.get(0)?,
                    author_id: row.get(1)?,
                    title: row.get(2)?,
                    tags: row.get(3)?,
                    created_at: row.get(4)?,
                    post_count: row.get(5)?,
                    last_post_at: row.get(6)?,
                })
            })
            .map_err(|_| StoreError::NotFound(format!("thread {}", object_id)))
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<ThreadRow>, StoreError> {
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, author_id, title, tags, created_at, post_count, last_post_at
                 FROM threads ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![limit, offset], |row| {
                    Ok(ThreadRow {
                        object_id: row.get(0)?,
                        author_id: row.get(1)?,
                        title: row.get(2)?,
                        tags: row.get(3)?,
                        created_at: row.get(4)?,
                        post_count: row.get(5)?,
                        last_post_at: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn delete(&self, object_id: &str) -> Result<(), StoreError> {
        let object_id = object_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute("DELETE FROM threads WHERE object_id = ?1", rusqlite::params![object_id])?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<ThreadRow>, StoreError> {
        let query = format!("%{}%", query);
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, author_id, title, tags, created_at, post_count, last_post_at
                 FROM threads WHERE title LIKE ?1 OR tags LIKE ?1 ORDER BY created_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![query, limit], |row| {
                    Ok(ThreadRow {
                        object_id: row.get(0)?,
                        author_id: row.get(1)?,
                        title: row.get(2)?,
                        tags: row.get(3)?,
                        created_at: row.get(4)?,
                        post_count: row.get(5)?,
                        last_post_at: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn increment_post_count(
        &self,
        thread_id: &str,
        post_time: i64,
    ) -> Result<(), StoreError> {
        let thread_id = thread_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "UPDATE threads SET post_count = post_count + 1, last_post_at = ?1 WHERE object_id = ?2",
                rusqlite::params![post_time, thread_id],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

// ─── Post repository ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostRow {
    pub object_id: String,
    pub thread_id: String,
    pub parent_id: Option<String>,
    pub author_id: String,
    pub body: String,
    pub created_at: i64,
    pub attachment_meta: Option<String>,
}

pub struct PostRepo<'a>(pub &'a Store);

impl<'a> PostRepo<'a> {
    pub async fn insert(&self, post: &hvoc_core::Post) -> Result<(), StoreError> {
        self.insert_with_attachment(post, None).await
    }

    pub async fn insert_with_attachment(
        &self,
        post: &hvoc_core::Post,
        attachment_meta: Option<String>,
    ) -> Result<(), StoreError> {
        let raw_json = serde_json::to_string(post)?;
        let object_id = post.object_id.clone();
        let thread_id = post.thread_id.clone();
        let parent_id = post.parent_id.clone();
        let author_id = post.author_id.clone();
        let body = post.body.clone();
        let created_at = post.created_at;
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO posts (object_id, thread_id, parent_id, author_id, body, created_at, attachment_meta, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![object_id, thread_id, parent_id, author_id, body, created_at, attachment_meta, raw_json],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn list_for_thread(&self, thread_id: &str) -> Result<Vec<PostRow>, StoreError> {
        let thread_id = thread_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, thread_id, parent_id, author_id, body, created_at, attachment_meta
                 FROM posts WHERE thread_id = ?1 AND tombstoned = 0 ORDER BY created_at ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![thread_id], |row| {
                    Ok(PostRow {
                        object_id: row.get(0)?,
                        thread_id: row.get(1)?,
                        parent_id: row.get(2)?,
                        author_id: row.get(3)?,
                        body: row.get(4)?,
                        created_at: row.get(5)?,
                        attachment_meta: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn get(&self, object_id: &str) -> Result<PostRow, StoreError> {
        let object_id = object_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, thread_id, parent_id, author_id, body, created_at, attachment_meta
                 FROM posts WHERE object_id = ?1",
            )?;
            stmt.query_row(rusqlite::params![object_id], |row| {
                Ok(PostRow {
                    object_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    author_id: row.get(3)?,
                    body: row.get(4)?,
                    created_at: row.get(5)?,
                    attachment_meta: row.get(6)?,
                })
            })
            .map_err(|_| StoreError::NotFound(format!("post {}", object_id)))
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

// ─── Message repository ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    pub object_id: String,
    pub sender_id: String,
    pub recipient_id: String,
    pub body: String,
    pub sent_at: i64,
    pub received_at: Option<i64>,
    pub direction: String,
    pub read: bool,
}

pub struct MessageRepo<'a>(pub &'a Store);

impl<'a> MessageRepo<'a> {
    pub async fn insert(
        &self,
        object_id: &str,
        sender_id: &str,
        recipient_id: &str,
        body: &str,
        sent_at: i64,
        received_at: Option<i64>,
        direction: &str,
        raw_envelope: &str,
    ) -> Result<(), StoreError> {
        let object_id = object_id.to_string();
        let sender_id = sender_id.to_string();
        let recipient_id = recipient_id.to_string();
        let body = body.to_string();
        let direction = direction.to_string();
        let raw_envelope = raw_envelope.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO messages (object_id, sender_id, recipient_id, body, sent_at, received_at, direction, raw_envelope)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![object_id, sender_id, recipient_id, body, sent_at, received_at, direction, raw_envelope],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn list_for_conversation(
        &self,
        my_id: &str,
        other_id: &str,
    ) -> Result<Vec<MessageRow>, StoreError> {
        let my_id = my_id.to_string();
        let other_id = other_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, sender_id, recipient_id, body, sent_at, received_at, direction, read
                 FROM messages
                 WHERE (sender_id = ?1 AND recipient_id = ?2) OR (sender_id = ?2 AND recipient_id = ?1)
                 ORDER BY sent_at ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![my_id, other_id], |row| {
                    Ok(MessageRow {
                        object_id: row.get(0)?,
                        sender_id: row.get(1)?,
                        recipient_id: row.get(2)?,
                        body: row.get(3)?,
                        sent_at: row.get(4)?,
                        received_at: row.get(5)?,
                        direction: row.get(6)?,
                        read: row.get::<_, i64>(7)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn list_all_for_user(
        &self,
        my_id: &str,
    ) -> Result<Vec<MessageRow>, StoreError> {
        let my_id = my_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT object_id, sender_id, recipient_id, body, sent_at, received_at, direction, read
                 FROM messages
                 WHERE sender_id = ?1 OR recipient_id = ?1
                 ORDER BY sent_at ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![my_id], |row| {
                    Ok(MessageRow {
                        object_id: row.get(0)?,
                        sender_id: row.get(1)?,
                        recipient_id: row.get(2)?,
                        body: row.get(3)?,
                        sent_at: row.get(4)?,
                        received_at: row.get(5)?,
                        direction: row.get(6)?,
                        read: row.get::<_, i64>(7)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

// ─── Contact repository ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactRow {
    pub author_id: String,
    pub nickname: Option<String>,
    pub added_at: i64,
    pub blocked: bool,
}

pub struct ContactRepo<'a>(pub &'a Store);

impl<'a> ContactRepo<'a> {
    pub async fn upsert(
        &self,
        author_id: &str,
        nickname: Option<&str>,
    ) -> Result<(), StoreError> {
        let author_id = author_id.to_string();
        let nickname = nickname.map(|s| s.to_string());
        let now = chrono::Utc::now().timestamp();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR IGNORE INTO contacts (author_id, nickname, added_at, blocked)
                 VALUES (?1, ?2, ?3, 0)",
                rusqlite::params![author_id, nickname, now],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn list(&self) -> Result<Vec<ContactRow>, StoreError> {
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT author_id, nickname, added_at, blocked FROM contacts ORDER BY added_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(ContactRow {
                        author_id: row.get(0)?,
                        nickname: row.get(1)?,
                        added_at: row.get(2)?,
                        blocked: row.get::<_, i64>(3)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

// ─── Identity repository (public profiles from DHT) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRow {
    pub author_id: String,
    pub handle: String,
    pub bio: String,
    pub public_key: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct IdentityRepo<'a>(pub &'a Store);

impl<'a> IdentityRepo<'a> {
    pub async fn upsert(
        &self,
        author_id: &str,
        handle: &str,
        bio: &str,
        public_key: &str,
        raw_json: &str,
    ) -> Result<(), StoreError> {
        let author_id = author_id.to_string();
        let handle = handle.to_string();
        let bio = bio.to_string();
        let public_key = public_key.to_string();
        let raw_json = raw_json.to_string();
        let now = chrono::Utc::now().timestamp();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO identities (author_id, handle, bio, public_key, created_at, updated_at, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
                 ON CONFLICT(author_id) DO UPDATE SET handle=?2, bio=?3, updated_at=?5, raw_json=?6",
                rusqlite::params![author_id, handle, bio, public_key, now, raw_json],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn get(&self, author_id: &str) -> Result<Option<IdentityRow>, StoreError> {
        let author_id = author_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || -> Result<Option<IdentityRow>, StoreError> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT author_id, handle, bio, public_key, created_at, updated_at
                 FROM identities WHERE author_id = ?1",
            )?;
            let result = stmt.query_row(rusqlite::params![author_id], |row| {
                Ok(IdentityRow {
                    author_id: row.get(0)?,
                    handle: row.get(1)?,
                    bio: row.get(2)?,
                    public_key: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
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

    /// Get handles for a batch of author IDs. Returns a map of author_id → handle.
    pub async fn get_handles(&self, author_ids: &[String]) -> Result<std::collections::HashMap<String, String>, StoreError> {
        if author_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let ids = author_ids.to_vec();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT author_id, handle FROM identities WHERE author_id IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<std::collections::HashMap<String, String>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

// ─── Tombstone repository ────────────────────────────────────────────────────

pub struct TombstoneRepo<'a>(pub &'a Store);

impl<'a> TombstoneRepo<'a> {
    pub async fn insert(&self, tombstone: &hvoc_core::Tombstone) -> Result<(), StoreError> {
        let raw_json = serde_json::to_string(tombstone)?;
        let object_id = tombstone.object_id.clone();
        let target_id = tombstone.target_id.clone();
        let author_id = tombstone.author_id.clone();
        let reason = tombstone.reason.clone();
        let created_at = tombstone.created_at;
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            // Insert the tombstone record.
            conn.execute(
                "INSERT OR IGNORE INTO tombstones (object_id, target_id, author_id, reason, created_at, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![object_id, target_id, author_id, reason, created_at, raw_json],
            )?;
            // Mark the target post as tombstoned.
            conn.execute(
                "UPDATE posts SET tombstoned = 1 WHERE object_id = ?1 AND author_id = ?2",
                rusqlite::params![target_id, author_id],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn is_tombstoned(&self, target_id: &str) -> Result<bool, StoreError> {
        let target_id = target_id.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tombstones WHERE target_id = ?1",
                rusqlite::params![target_id],
                |row| row.get(0),
            )?;
            Ok::<_, StoreError>(count > 0)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}

// ─── Board index repository (thread discovery) ──────────────────────────────

pub struct BoardRepo<'a>(pub &'a Store);

impl<'a> BoardRepo<'a> {
    pub async fn add_thread(
        &self,
        board_name: &str,
        thread_dht_key: &str,
        thread_id: &str,
    ) -> Result<(), StoreError> {
        let board_name = board_name.to_string();
        let thread_dht_key = thread_dht_key.to_string();
        let thread_id = thread_id.to_string();
        let now = chrono::Utc::now().timestamp();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR IGNORE INTO board_index (board_name, thread_dht_key, thread_id, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![board_name, thread_dht_key, thread_id, now],
            )?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }

    pub async fn list_threads(&self, board_name: &str) -> Result<Vec<(String, String)>, StoreError> {
        let board_name = board_name.to_string();
        let conn = self.0.conn().clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT thread_dht_key, thread_id FROM board_index WHERE board_name = ?1 ORDER BY added_at DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![board_name], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, StoreError>(rows)
        })
        .await
        .map_err(|e| StoreError::Task(e.to_string()))?
    }
}
