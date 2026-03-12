//! Integration tests for hvoc-store repositories.

use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

async fn temp_store() -> hvoc_store::Store {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hvoc-test-{}-{}",
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("test.db");
    hvoc_store::Store::open(&db).await.unwrap()
}

#[tokio::test]
async fn thread_insert_and_get() {
    let store = temp_store().await;
    let repo = hvoc_store::ThreadRepo(&store);

    let thread = hvoc_core::Thread::new(
        "author1".into(),
        "Test Thread".into(),
        1000,
        vec!["rust".into()],
        hvoc_core::Thread::compute_id("author1", "Test Thread", 1000, &["rust".into()]).unwrap(),
        vec![0u8; 64],
    );
    repo.insert(&thread).await.unwrap();

    let fetched = repo.get(&thread.object_id).await.unwrap();
    assert_eq!(fetched.title, "Test Thread");
    assert_eq!(fetched.author_id, "author1");
    assert_eq!(fetched.post_count, 0);
}

#[tokio::test]
async fn thread_list_with_pagination() {
    let store = temp_store().await;
    let repo = hvoc_store::ThreadRepo(&store);

    for i in 0..5 {
        let thread = hvoc_core::Thread::new(
            "author1".into(),
            format!("Thread {}", i),
            1000 + i,
            vec![],
            hvoc_core::Thread::compute_id("author1", &format!("Thread {}", i), 1000 + i, &[]).unwrap(),
            vec![0u8; 64],
        );
        repo.insert(&thread).await.unwrap();
    }

    let all = repo.list(10, 0).await.unwrap();
    assert_eq!(all.len(), 5);

    let page = repo.list(2, 0).await.unwrap();
    assert_eq!(page.len(), 2);

    let page2 = repo.list(2, 2).await.unwrap();
    assert_eq!(page2.len(), 2);
}

#[tokio::test]
async fn thread_search() {
    let store = temp_store().await;
    let repo = hvoc_store::ThreadRepo(&store);

    let t1 = hvoc_core::Thread::new(
        "a".into(), "Rust Programming".into(), 1, vec!["rust".into()],
        hvoc_core::Thread::compute_id("a", "Rust Programming", 1, &["rust".into()]).unwrap(),
        vec![0u8; 64],
    );
    let t2 = hvoc_core::Thread::new(
        "a".into(), "Python Tips".into(), 2, vec!["python".into()],
        hvoc_core::Thread::compute_id("a", "Python Tips", 2, &["python".into()]).unwrap(),
        vec![0u8; 64],
    );
    repo.insert(&t1).await.unwrap();
    repo.insert(&t2).await.unwrap();

    let results = repo.search("Rust", 10).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Rust Programming");

    let results = repo.search("python", 10).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn thread_delete() {
    let store = temp_store().await;
    let repo = hvoc_store::ThreadRepo(&store);

    let thread = hvoc_core::Thread::new(
        "a".into(), "Delete Me".into(), 1, vec![],
        hvoc_core::Thread::compute_id("a", "Delete Me", 1, &[]).unwrap(),
        vec![0u8; 64],
    );
    repo.insert(&thread).await.unwrap();
    repo.delete(&thread.object_id).await.unwrap();

    let result = repo.get(&thread.object_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn post_insert_and_list() {
    let store = temp_store().await;
    let post_repo = hvoc_store::PostRepo(&store);

    let post = hvoc_core::Post::new(
        "author1".into(),
        "thread1".into(),
        None,
        "Hello world".into(),
        2000,
        hvoc_core::Post::compute_id("author1", "thread1", None, "Hello world", 2000).unwrap(),
        vec![0u8; 64],
    );
    post_repo.insert(&post).await.unwrap();

    let posts = post_repo.list_for_thread("thread1").await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].body, "Hello world");
}

#[tokio::test]
async fn contact_upsert_and_list() {
    let store = temp_store().await;
    let repo = hvoc_store::ContactRepo(&store);

    repo.upsert("pubkey1", Some("Alice")).await.unwrap();
    repo.upsert("pubkey2", None).await.unwrap();

    let contacts = repo.list().await.unwrap();
    assert_eq!(contacts.len(), 2);
    assert!(contacts.iter().any(|c| c.author_id == "pubkey1" && c.nickname.as_deref() == Some("Alice")));
}

#[tokio::test]
async fn contact_upsert_is_idempotent() {
    let store = temp_store().await;
    let repo = hvoc_store::ContactRepo(&store);

    repo.upsert("pubkey1", Some("Alice")).await.unwrap();
    repo.upsert("pubkey1", Some("Alice2")).await.unwrap(); // should NOT update (INSERT OR IGNORE)

    let contacts = repo.list().await.unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].nickname.as_deref(), Some("Alice")); // original kept
}

#[tokio::test]
async fn message_insert_and_query() {
    let store = temp_store().await;
    let repo = hvoc_store::MessageRepo(&store);

    repo.insert("msg1", "alice", "bob", "hello", 1000, None, "sent", "{}").await.unwrap();
    repo.insert("msg2", "bob", "alice", "hi back", 1001, Some(1001), "received", "{}").await.unwrap();

    let conv = repo.list_for_conversation("alice", "bob").await.unwrap();
    assert_eq!(conv.len(), 2);
    assert_eq!(conv[0].body, "hello");
    assert_eq!(conv[1].body, "hi back");
}

#[tokio::test]
async fn identity_upsert_and_get() {
    let store = temp_store().await;
    let repo = hvoc_store::IdentityRepo(&store);

    repo.upsert("author1", "handle1", "bio", "pubkey1", "{}").await.unwrap();

    let id = repo.get("author1").await.unwrap();
    assert!(id.is_some());
    let id = id.unwrap();
    assert_eq!(id.handle, "handle1");
    assert_eq!(id.bio, "bio");
}

#[tokio::test]
async fn identity_batch_handles() {
    let store = temp_store().await;
    let repo = hvoc_store::IdentityRepo(&store);

    repo.upsert("a1", "Alice", "", "k1", "{}").await.unwrap();
    repo.upsert("a2", "Bob", "", "k2", "{}").await.unwrap();

    let handles = repo.get_handles(&["a1".into(), "a2".into(), "a3".into()]).await.unwrap();
    assert_eq!(handles.get("a1").unwrap(), "Alice");
    assert_eq!(handles.get("a2").unwrap(), "Bob");
    assert!(!handles.contains_key("a3"));
}

#[tokio::test]
async fn tombstone_marks_post() {
    let store = temp_store().await;
    let post_repo = hvoc_store::PostRepo(&store);
    let tomb_repo = hvoc_store::TombstoneRepo(&store);

    let post = hvoc_core::Post::new(
        "author1".into(), "thread1".into(), None, "Delete me".into(), 1000,
        hvoc_core::Post::compute_id("author1", "thread1", None, "Delete me", 1000).unwrap(),
        vec![0u8; 64],
    );
    post_repo.insert(&post).await.unwrap();

    let tombstone = hvoc_core::Tombstone::new(
        "author1".into(), post.object_id.clone(), Some("test".into()), 2000,
        hvoc_core::Tombstone::compute_id("author1", &post.object_id, Some("test"), 2000).unwrap(),
        vec![0u8; 64],
    );
    tomb_repo.insert(&tombstone).await.unwrap();

    // Post should now be tombstoned — not appear in thread listing.
    let posts = post_repo.list_for_thread("thread1").await.unwrap();
    assert_eq!(posts.len(), 0);

    assert!(tomb_repo.is_tombstoned(&post.object_id).await.unwrap());
}

#[tokio::test]
async fn board_index() {
    let store = temp_store().await;
    let repo = hvoc_store::BoardRepo(&store);

    repo.add_thread("default", "dht_key_1", "thread_1").await.unwrap();
    repo.add_thread("default", "dht_key_2", "thread_2").await.unwrap();

    let threads = repo.list_threads("default").await.unwrap();
    assert_eq!(threads.len(), 2);
}

#[tokio::test]
async fn keystore_save_and_load() {
    let store = temp_store().await;
    let ks = hvoc_store::Keystore(&store);

    let secret = vec![42u8; 64];
    ks.save("id1", "handle1", &secret, b"password123").await.unwrap();

    let loaded = ks.load("id1", b"password123").await.unwrap();
    assert_eq!(loaded, secret);

    let ids = ks.list_ids().await.unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].handle, "handle1");
}

#[tokio::test]
async fn keystore_wrong_password() {
    let store = temp_store().await;
    let ks = hvoc_store::Keystore(&store);

    let secret = vec![42u8; 64];
    ks.save("id1", "handle1", &secret, b"correct").await.unwrap();

    let loaded = ks.load("id1", b"wrong").await.unwrap();
    // XOR-based "encryption" will return garbled data, not an error.
    assert_ne!(loaded, secret);
}
