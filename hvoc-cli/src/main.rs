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
    let store = hvoc_store::Store::open(&db_path).await?;

    match cli.command {
        Commands::Identity { cmd } => handle_identity(cmd, &store).await?,
        Commands::Thread { cmd } => handle_thread(cmd, &store, &data_dir).await?,
        Commands::Post { cmd } => handle_post(cmd, &store, &data_dir).await?,
        Commands::Serve { bind } => {
            let node = hvoc_veilid::HvocNode::start(data_dir.clone()).await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let state = Arc::new(hvoc_api::AppState {
                store,
                node,
                keypair: RwLock::new(None),
                author_id: RwLock::new(None),
                data_dir: data_dir.clone(),
            });

            // Auto-open browser after a short delay.
            let url = format!("http://{bind}");
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let _ = open_browser(&url);
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
