use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// ilearned: lightweight AI agent memory management - a live rule book of learned experiences (when/if/do/check) grouped by topic. Use `search` to find applicable rules, `add` to record new lessons, `modify` to refine them, `promote`/`downgrade` for feedback, `delete`/`clear` for removal.
#[derive(Debug, Parser)]
#[command(name = "ilearned", version)]
pub struct Cli {
    /// SQLite database path (env ILEARNED_DB, default ./ilearned.db,
    /// ./.ilearned/ilearned.db for `mcp`).
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

/// Shared topic help: hierarchical form + wildcard note for search filters.
pub const TOPIC_HELP: &str = "hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; search accepts # multi-level wildcard";

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Add a new experience (sets good_count=1, state=active).
    Add(AddArgs),
    /// Search experiences by topic, full-text, or semantic query.
    Search(SearchArgs),
    /// Modify an existing experience (at least one field required).
    Modify(ModifyArgs),
    /// Soft-delete an experience (idempotent on already-deleted).
    Delete(DeleteArgs),
    /// Promote an experience (increments good_count, restores to active).
    Promote(IdArgs),
    /// Downgrade an experience (increments bad_count).
    Downgrade(IdArgs),
    /// Clear experiences by topic or all (destructive, needs confirmation).
    Clear(ClearArgs),
    /// List/search existing topics (hierarchical, paginated).
    Topic(TopicArgs),
    /// Export experiences as JSONL (stdout, or --file PATH).
    Export(ExportArgs),
    /// Import experiences from JSONL (--file PATH or stdin).
    Import(ImportArgs),
    /// Start REST + web + MCP on one listener.
    Serve(ServeArgs),
    /// Run as an MCP server over stdio (stdin/stdout) for harness use.
    /// DB defaults to `./.ilearned/ilearned.db` when `--db`/`ILEARNED_DB`
    /// are unset; stdout stays pure JSON-RPC (logs go to stderr).
    Mcp,
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; search accepts # multi-level wildcard.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: String,
    /// Scenario this experience applies to, including context, conditions, and constraints.
    #[arg(long = "when")]
    pub when: String,
    /// Trigger(s), e.g. something happened, observed, or detected.
    #[arg(long = "if")]
    pub if_: String,
    /// Action(s) the agent should take / try, include steps, procedures, and instructions.
    #[arg(long = "do")]
    pub do_: String,
    /// Signal(s) to verify the experience was useful, list what to look for to confirm the scenario matches, how to identify triggers, and what can be used to confirm the results of the action.
    #[arg(long)]
    pub check: String,
}

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    /// Hierarchical topic filter, e.g. travel/hotel/checkout; # examples: travel/#, #/checkout. Segments [a-z0-9_-], '/' separated; search accepts # multi-level wildcard.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: Option<String>,
    /// FTS5 full-text query.
    #[arg(long)]
    pub text: Option<String>,
    /// Semantic query (requires embedding provider, else exit 3).
    #[arg(long)]
    pub semantic: Option<String>,
    /// Max results (default 20).
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
    /// Result offset (default 0).
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    /// Include inactive records (deleted/forgotten always excluded).
    #[arg(long)]
    pub deep: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ModifyArgs {
    /// Hierarchical topic of the record to modify.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: String,
    /// Id of the record to modify.
    #[arg(long)]
    pub id: String,
    /// New scenario text (blank values ignored).
    #[arg(long = "when")]
    pub when: Option<String>,
    /// New trigger text (blank values ignored).
    #[arg(long = "if")]
    pub if_: Option<String>,
    /// New action text (blank values ignored).
    #[arg(long = "do")]
    pub do_: Option<String>,
    /// New check text (blank values ignored).
    #[arg(long)]
    pub check: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct DeleteArgs {
    /// Hierarchical topic of the record to delete.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: String,
    /// Id of the record to delete.
    #[arg(long)]
    pub id: String,
    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone, Args)]
pub struct IdArgs {
    /// Hierarchical topic of the record.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: String,
    /// Id of the record.
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Clone, Args)]
pub struct ClearArgs {
    /// Clear a single topic (exact match; no wildcards).
    #[arg(long, conflicts_with = "all", required_unless_present = "all", help = TOPIC_HELP)]
    pub topic: Option<String>,
    /// Clear all topics.
    #[arg(long, conflicts_with = "topic", required_unless_present = "topic")]
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

#[derive(Debug, Clone, Args)]
pub struct ExportArgs {
    /// Hierarchical topic filter; search accepts # multi-level wildcard.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: Option<String>,
    /// Include inactive records (deleted/forgotten always excluded).
    #[arg(long)]
    pub deep: bool,
    /// Write to PATH instead of stdout (parent dirs created, overwritten).
    #[arg(long)]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ImportArgs {
    /// Read from PATH instead of stdin.
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// Keep file ids and overwrite on (topic, id) collision.
    /// Without --merge every line gets a fresh id.
    #[arg(long)]
    pub merge: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TopicArgs {
    #[command(subcommand)]
    pub command: TopicCommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TopicCommands {
    /// List existing topics, optionally truncated to --level depth.
    List(TopicListArgs),
    /// Search topics by substring or # wildcard pattern.
    Search(TopicSearchArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TopicListArgs {
    /// Limit hierarchy depth (e.g. --level 2 shows travel/hotel, not travel/hotel/checkout).
    #[arg(long)]
    pub level: Option<u32>,
    /// Max topics (default 20).
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
    /// Result offset (default 0).
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    /// Include topics that only have inactive records.
    #[arg(long)]
    pub deep: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TopicSearchArgs {
    /// Substring or # multi-level wildcard pattern (e.g. hotel, travel/#, #/checkout).
    pub query: String,
    /// Limit hierarchy depth (applied after matching).
    #[arg(long)]
    pub level: Option<u32>,
    /// Max topics (default 20).
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
    /// Result offset (default 0).
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    /// Include topics that only have inactive records.
    #[arg(long)]
    pub deep: bool,
}
