#![recursion_limit = "256"]
//! HVOC CLI — entrypoint for identity management, forum ops, and API server.
//!
//! Usage:
//!   hvoc identity create --handle d8rh8r
//!   hvoc identity list
//!   hvoc thread create --title "Test" --body "Hello Veilid"
//!   hvoc thread list
//!   hvoc post create --thread <id> --body "Reply"
//!   hvoc post list --thread <id>
//!   hvoc serve

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "hvoc", about = "HVOC — Veilid P2P forum + messaging")]
struct Cli {
    /// Data directory (default: ~/.hvoc)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Identity {
        #[command(subcommand)]
        cmd: IdentityCmd,
    },
    Thread {
        #[command(subcommand)]
        cmd: ThreadCmd,
    },
    Post {
        #[command(subcommand)]
        cmd: PostCmd,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:7734")]
        bind: SocketAddr,
    },
}

#[derive(Subcommand)]
enum IdentityCmd {
    Create {
        #[arg(long)]
        handle: String,
    },
    List,
}

#[derive(Subcommand)]
enum ThreadCmd {
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        tags: Vec<String>,
    },
    List,
    Show {
        id: String,
    },
}

#[derive(Subcommand)]
enum PostCmd {
    Create {
        #[arg(long)]
        thread: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        parent: Option<String>,
    },
    List {
        #[arg(long)]
        thread: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("hvoc=info".parse()?)
                .add_directive("veilid_core=warn".parse()?)
                .add_directive("veilid_api=warn".parse()?)
                .add_directive("net=off".parse()?)
                .add_directive("protocol=off".parse()?),
        )
        .compact()
        .init();

    let cli = Cli::parse();

    let data_dir = cli.data_dir.unwrap_or_else(|| {
        dirs_next::home_dir()
            .unwrap_or_default()
            .join(".hvoc")
    });
    std::fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("hvoc.db");

    match cli.command {
        Commands::Identity { cmd } => {
            let store = hvoc_store::Store::open(&db_path).await?;
            handle_identity(cmd, &store).await?;
        }
        Commands::Thread { cmd } => {
            let store = hvoc_store::Store::open(&db_path).await?;
            handle_thread(cmd, &store, &data_dir).await?;
        }
        Commands::Post { cmd } => {
            let store = hvoc_store::Store::open(&db_path).await?;
            handle_post(cmd, &store, &data_dir).await?;
        }
        Commands::Serve { bind } => {
            let node = hvoc_veilid::HvocNode::start(data_dir.clone()).await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            // If plaintext DB is missing but encrypted snapshot exists, restore it.
            let enc_path = data_dir.join("hvoc.db.enc");
            if !db_path.exists() && enc_path.exists() {
                let db_key = load_or_create_db_key(&node)?;
                let enc_data = std::fs::read(&enc_path)?;
                match node.with_crypto(|cs| {
                    hvoc_veilid::crypto::decrypt_blob(cs, &db_key, &enc_data)
                }) {
                    Ok(plain) => {
                        std::fs::write(&db_path, &plain)?;
                        tracing::info!("Database restored from encrypted snapshot");
                    }
                    Err(e) => {
                        anyhow::bail!("Failed to decrypt database: {e}");
                    }
                }
            }

            let store = hvoc_store::Store::open(&db_path).await?;

            // Bootstrap seed threads on first run.
            if let Err(e) = hvoc_store::bootstrap::bootstrap_if_needed(&store).await {
                tracing::warn!("Bootstrap failed: {e}");
            }

            let state = Arc::new(hvoc_api::AppState {
                store,
                node,
                keypair: RwLock::new(None),
                author_id: RwLock::new(None),
                data_dir: data_dir.clone(),
                call_state: RwLock::new(hvoc_api::CallState {
                    active_peer: None,
                    started_at: None,
                }),
            });

            // Auto-open browser after a short delay.
            let url = format!("http://{bind}");
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let _ = open_browser(&url);
            });

            // Periodically encrypt DB at rest (every 60s).
            // This ensures an encrypted copy exists even after a hard kill.
            let enc_state = state.clone();
            let enc_db_path = db_path.clone();
            let enc_enc_path = data_dir.join("hvoc.db.enc");
            tokio::spawn(async move {
                // Wait for initial setup to complete.
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                loop {
                    // Checkpoint WAL so all data is in the main DB file.
                    if let Err(e) = enc_state.store.checkpoint().await {
                        tracing::warn!("WAL checkpoint failed: {e}");
                    }
                    encrypt_db_snapshot(&enc_state.node, &enc_db_path, &enc_enc_path);
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }
            });

            hvoc_api::serve(state, bind).await?;
        }
    }

    Ok(())
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

async fn handle_identity(cmd: IdentityCmd, store: &hvoc_store::Store) -> Result<()> {
    match cmd {
        IdentityCmd::Create { handle } => {
            // For CLI, we need a running node to access crypto.
            // For Phase 1, generate a placeholder identity locally.
            println!("Identity creation requires a running node.");
            println!("Use `hvoc serve` and create via the API, or run:");
            println!("  curl -X POST http://127.0.0.1:7734/api/identity \\");
            println!("    -H 'Content-Type: application/json' \\");
            println!("    -d '{{\"handle\": \"{handle}\", \"passphrase\": \"your-passphrase\"}}'");
        }
        IdentityCmd::List => {
            let ks = hvoc_store::Keystore(store);
            let ids = ks.list_ids().await.map_err(|e| anyhow::anyhow!("{e}"))?;
            if ids.is_empty() {
                println!("No identities found.");
            } else {
                println!("{} identity(ies):", ids.len());
                for id in ids {
                    println!("  {} ({})", id.handle, id.id);
                }
            }
        }
    }
    Ok(())
}

async fn handle_thread(
    cmd: ThreadCmd,
    store: &hvoc_store::Store,
    _data_dir: &PathBuf,
) -> Result<()> {
    let repo = hvoc_store::ThreadRepo(store);

    match cmd {
        ThreadCmd::Create { title, body, tags } => {
            println!("Thread creation requires a running node for signing.");
            println!("Use `hvoc serve` and create via the API.");
            let _ = (title, body, tags);
        }
        ThreadCmd::List => {
            let threads = repo.list(20, 0).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            if threads.is_empty() {
                println!("No threads.");
            } else {
                for t in threads {
                    println!(
                        "[{}] {} ({} posts)",
                        &t.object_id[..8.min(t.object_id.len())],
                        t.title,
                        t.post_count
                    );
                }
            }
        }
        ThreadCmd::Show { id } => {
            let t = repo.get(&id).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("Thread: {}", t.title);
            println!("ID:     {}", t.object_id);
            println!("Author: {}", t.author_id);
            println!("Posts:  {}", t.post_count);
        }
    }
    Ok(())
}

async fn handle_post(
    cmd: PostCmd,
    store: &hvoc_store::Store,
    _data_dir: &PathBuf,
) -> Result<()> {
    let repo = hvoc_store::PostRepo(store);

    match cmd {
        PostCmd::Create { thread, body, parent } => {
            println!("Post creation requires a running node for signing.");
            println!("Use `hvoc serve` and create via the API.");
            let _ = (thread, body, parent);
        }
        PostCmd::List { thread } => {
            let posts = repo
                .list_for_thread(&thread)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if posts.is_empty() {
                println!("No posts.");
            } else {
                for p in posts {
                    let preview: String = p.body.chars().take(60).collect();
                    println!(
                        "[{}] {}: {}",
                        &p.object_id[..8.min(p.object_id.len())],
                        &p.author_id[..8.min(p.author_id.len())],
                        preview
                    );
                }
            }
        }
    }
    Ok(())
}

// ─── DB encryption helpers ──────────────────────────────────────────────────

const DB_KEY_NAME: &str = "hvoc-db-key";

/// Load or create the DB encryption key from Veilid's protected store.
fn load_or_create_db_key(
    node: &hvoc_veilid::HvocNode,
) -> Result<veilid_core::SharedSecret> {
    let ps = node.api.protected_store()
        .map_err(|e| anyhow::anyhow!("protected store: {e}"))?;

    if let Some(key_bytes) = ps.load_user_secret(DB_KEY_NAME)
        .map_err(|e| anyhow::anyhow!("load db key: {e}"))? {
        if key_bytes.len() == 32 {
            use std::convert::TryFrom;
            let bare = veilid_core::BareSharedSecret::try_from(key_bytes.as_slice())
                .map_err(|e| anyhow::anyhow!("invalid db key: {e}"))?;
            return Ok(veilid_core::SharedSecret::new(veilid_core::CRYPTO_KIND_VLD0, bare));
        }
    }

    // Generate new key.
    let key = node.with_crypto(|cs| {
        Ok(hvoc_veilid::crypto::generate_db_key(cs))
    }).map_err(|e| anyhow::anyhow!("generate db key: {e}"))?;

    ps.save_user_secret(DB_KEY_NAME, key.value().bytes())
        .map_err(|e| anyhow::anyhow!("save db key: {e}"))?;

    tracing::info!("Generated new database encryption key");
    Ok(key)
}

/// Encrypt the DB to a .enc snapshot (keeps plaintext since server is still running).
/// On next startup, if .enc exists and .db doesn't, it decrypts from .enc.
fn encrypt_db_snapshot(
    node: &std::sync::Arc<hvoc_veilid::HvocNode>,
    db_path: &std::path::Path,
    enc_path: &std::path::Path,
) {
    let db_key = match load_or_create_db_key(node) {
        Ok(k) => k,
        Err(e) => {
            tracing::error!("Cannot encrypt DB — failed to load key: {e}");
            return;
        }
    };

    // Read the WAL-consolidated DB. SQLite may have data in the WAL file,
    // so we read the main DB + let SQLite handle it on next open.
    let plain = match std::fs::read(db_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Cannot read DB for encryption: {e}");
            return;
        }
    };

    match node.with_crypto(|cs| {
        hvoc_veilid::crypto::encrypt_blob(cs, &db_key, &plain)
    }) {
        Ok(encrypted) => {
            if let Err(e) = std::fs::write(enc_path, &encrypted) {
                tracing::error!("Failed to write encrypted DB: {e}");
                return;
            }
            tracing::info!("Database snapshot encrypted ({} bytes)", encrypted.len());
        }
        Err(e) => {
            tracing::error!("DB encryption failed: {e}");
        }
    }
}
