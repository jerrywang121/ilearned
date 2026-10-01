use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Stable JSON output for agents and scripts; human-readable output is the default.
#[derive(Debug, Clone, Args, Default)]
pub struct JsonArgs {
    /// Stable JSON output for agents and scripts.
    #[arg(long)]
    pub json: bool,
}

/// ilearned: lightweight AI agent memory management - a live rule book of learned experiences (when/if/do/check) grouped by topic. Use `search` to find applicable rules, `add` to record new lessons, `update` to refine them, `promote`/`demote` for feedback, `delete`/`clear` for removal.
#[derive(Debug, Parser)]
#[command(name = "ilearned", version)]
pub struct Cli {
    /// TOML configuration file to overlay global and local configuration.
    #[arg(long, global = true, value_name = "PATH")]
    pub config_file: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Commands,
}

/// Shared topic help: canonical hierarchical form for write paths.
pub const TOPIC_HELP: &str =
    "hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated";

/// Topic filter help for search/export: canonical form plus `#` wildcards.
pub const TOPIC_FILTER_HELP: &str = "hierarchical topic filter, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; # is a multi-level wildcard (travel/#, #/checkout, travel/#/checkout); bare travel matches exact only";

/// Exact-match topic help for destructive paths (no wildcards accepted).
pub const TOPIC_EXACT_HELP: &str =
    "hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; exact match only, no wildcards";

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Add a new experience (sets good_count=1, state=active).
    Add(AddArgs),
    /// Search experiences by topic, full-text, or semantic query.
    Search(SearchArgs),
    /// Update an existing experience (at least one field required).
    Update(UpdateArgs),
    /// Soft-delete an experience (idempotent on already-deleted).
    Delete(DeleteArgs),
    /// Promote an experience (increments good_count, restores to active).
    Promote(IdArgs),
    /// Demote an experience (increments bad_count).
    Demote(IdArgs),
    /// Clear experiences by topic or all (destructive, needs confirmation).
    Clear(ClearArgs),
    /// List/search existing topics (hierarchical, paginated).
    Topic(TopicArgs),
    /// Export experiences as JSONL (stdout, or --file PATH).
    Export(ExportArgs),
    /// Import experiences from JSONL (--file PATH or stdin).
    Import(ImportArgs),
    /// Inspect or initialize TOML configuration files.
    Config(ConfigArgs),
    /// Start REST + web + MCP on one listener.
    Serve(ServeArgs),
    /// Run as an MCP server over stdio (stdin/stdout) for harness use.
    Mcp(McpArgs),
}

impl Commands {
    pub fn json(&self) -> bool {
        match self {
            Self::Add(a) => a.output.json,
            Self::Search(a) => a.output.json,
            Self::Update(a) => a.output.json,
            Self::Delete(a) => a.output.json,
            Self::Promote(a) | Self::Demote(a) => a.output.json,
            Self::Clear(a) => a.output.json,
            Self::Topic(a) => match &a.command {
                TopicCommands::List(a) => a.output.json,
                TopicCommands::Search(a) => a.output.json,
            },
            Self::Export(_) => false,
            Self::Import(a) => a.output.json,
            Self::Config(a) => matches!(&a.command, ConfigCommands::Show),
            Self::Serve(_) | Self::Mcp(_) => false,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Hierarchical topic, e.g. travel/hotel/checkout.
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
    #[command(flatten)]
    pub output: JsonArgs,
}

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    /// Hierarchical topic filter; # examples: travel/#, #/checkout.
    #[arg(long, help = TOPIC_FILTER_HELP)]
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
    #[command(flatten)]
    pub output: JsonArgs,
}

#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    /// Hierarchical topic of the record to update.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: String,
    /// Id of the record to update.
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
    #[command(flatten)]
    pub output: JsonArgs,
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
    #[command(flatten)]
    pub output: JsonArgs,
}

#[derive(Debug, Clone, Args)]
pub struct IdArgs {
    /// Hierarchical topic of the record.
    #[arg(long, help = TOPIC_HELP)]
    pub topic: String,
    /// Id of the record.
    #[arg(long)]
    pub id: String,
    #[command(flatten)]
    pub output: JsonArgs,
}

#[derive(Debug, Clone, Args)]
pub struct ClearArgs {
    /// Clear a single topic (exact match; no wildcards).
    #[arg(long, conflicts_with = "all", required_unless_present = "all", help = TOPIC_EXACT_HELP)]
    pub topic: Option<String>,
    /// Clear all topics.
    #[arg(long, conflicts_with = "topic", required_unless_present = "topic")]
    pub all: bool,
    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub yes: bool,
    #[command(flatten)]
    pub output: JsonArgs,
}

#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    /// Bind address override; takes precedence over config files and environment.
    #[arg(long)]
    pub bind: Option<SocketAddr>,
}

#[derive(Debug, Clone, Args)]
pub struct McpArgs {}

#[derive(Debug, Clone, Args)]
pub struct ExportArgs {
    /// Hierarchical topic filter; # examples: travel/#, #/checkout.
    #[arg(long, help = TOPIC_FILTER_HELP)]
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
    #[command(flatten)]
    pub output: JsonArgs,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommands {
    /// Show the resolved configuration and the files used to resolve it.
    Show,
    /// Create a default configuration file.
    Init(ConfigInitArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigInitArgs {
    /// Write the global configuration instead of the project-local one.
    #[arg(short = 'g', long = "global")]
    pub global: bool,
    /// Overwrite an existing configuration file.
    #[arg(short = 'f', long = "force")]
    pub force: bool,
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
    #[command(flatten)]
    pub output: JsonArgs,
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
    #[command(flatten)]
    pub output: JsonArgs,
}
