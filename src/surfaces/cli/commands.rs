use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// ilearned: lightweight AI agent memory management.
#[derive(Debug, Parser)]
#[command(name = "ilearned", version)]
pub struct Cli {
    /// SQLite database path (env ILEARNED_DB, default ./ilearned.db).
    #[arg(long, global = true, env = "ILEARNED_DB")]
    pub db: Option<PathBuf>,
    /// Bind address for `serve` (env ILEARNED_BIND).
    #[arg(long, global = true, env = "ILEARNED_BIND")]
    pub bind: Option<SocketAddr>,
    /// Days before active records go inactive (env ILEARNED_ACTIVE_DAYS, default 60).
    #[arg(long, global = true, env = "ILEARNED_ACTIVE_DAYS")]
    pub active_days: Option<u64>,
    /// Days before records are forgotten (env ILEARNED_FORGET_DAYS, default 120).
    #[arg(long, global = true, env = "ILEARNED_FORGET_DAYS")]
    pub forget_days: Option<u64>,
    /// Days before deleted/forgotten rows purge (env ILEARNED_RETENTION_DAYS, default 60).
    #[arg(long, global = true, env = "ILEARNED_RETENTION_DAYS")]
    pub retention_days: Option<u64>,
    /// OpenAI-compatible embeddings base URL (env ILEARNED_EMBED_ENDPOINT).
    /// Semantic search is enabled only when endpoint + model + key resolve.
    #[arg(long, global = true, env = "ILEARNED_EMBED_ENDPOINT")]
    pub embed_endpoint: Option<String>,
    /// Embedding model id (env ILEARNED_EMBED_MODEL).
    #[arg(long, global = true, env = "ILEARNED_EMBED_MODEL")]
    pub embed_model: Option<String>,
    /// Embedding API key (env ILEARNED_EMBED_API_KEY).
    #[arg(long, global = true, env = "ILEARNED_EMBED_API_KEY")]
    pub embed_api_key: Option<String>,
    /// Embedding vector dims (env ILEARNED_EMBED_DIMS).
    #[arg(long, global = true, env = "ILEARNED_EMBED_DIMS")]
    pub embed_dims: Option<usize>,
    /// Embedding HTTP timeout secs (env ILEARNED_EMBED_TIMEOUT_SECS).
    #[arg(long, global = true, env = "ILEARNED_EMBED_TIMEOUT_SECS")]
    pub embed_timeout_secs: Option<u64>,
    /// Stable JSON output (agents/scripts); default is human-readable.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    Add(AddArgs),
    Search(SearchArgs),
    Modify(ModifyArgs),
    Delete(DeleteArgs),
    Promote(IdArgs),
    Downgrade(IdArgs),
    Clear(ClearArgs),
    Serve(ServeArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    #[arg(long)]
    pub topic: String,
    #[arg(long = "when")]
    pub when: String,
    #[arg(long = "if")]
    pub if_: String,
    #[arg(long = "do")]
    pub do_: String,
    #[arg(long)]
    pub check: String,
}

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    #[arg(long)]
    pub topic: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub semantic: Option<String>,
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    #[arg(long)]
    pub deep: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ModifyArgs {
    #[arg(long)]
    pub topic: String,
    #[arg(long)]
    pub id: String,
    #[arg(long = "when")]
    pub when: Option<String>,
    #[arg(long = "if")]
    pub if_: Option<String>,
    #[arg(long = "do")]
    pub do_: Option<String>,
    #[arg(long)]
    pub check: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct DeleteArgs {
    #[arg(long)]
    pub topic: String,
    #[arg(long)]
    pub id: String,
    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone, Args)]
pub struct IdArgs {
    #[arg(long)]
    pub topic: String,
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Clone, Args)]
#[group(required = true, multiple = false)]
pub struct ClearArgs {
    #[arg(long)]
    pub topic: Option<String>,
    #[arg(long)]
    pub all: bool,
    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    /// Bind address override (default from config).
    #[arg(long)]
    pub bind: Option<SocketAddr>,
}
