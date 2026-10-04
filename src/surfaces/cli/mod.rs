pub mod commands;
pub mod render;

pub use commands::{Cli, Commands};
pub use render::{
    confirm_destructive, exit_code, render_clear, render_database_encryption,
    render_embedding_migration, render_error, render_feedback, render_identity, render_list,
};

use crate::application::MemoryService;
use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, SearchQuery, TopicQuery, UpdateCommand,
};
use crate::error::AppError;
use crate::storage::embeddings::VectorStore;
use crate::storage::repository::ExperienceRepo;

/// Execute one CLI command. Returns the process exit code (0 = ok).
/// Errors are rendered by the caller (main) so `--json` error shape is uniform.
pub fn run_cli<R: ExperienceRepo + VectorStore>(
    svc: &MemoryService<R>,
    cmd: &Commands,
    json: bool,
) -> Result<String, AppError> {
    match cmd {
        Commands::Add(a) => {
            let e = svc.add(AddCommand {
                topic: a.topic.clone(),
                when_text: a.when.clone(),
                if_text: a.if_.clone(),
                do_text: a.do_.clone(),
                check_text: a.check.clone(),
            })?;
            Ok(render_identity(&e.topic, &e.id, json, "added"))
        }
        Commands::Search(a) => {
            let out = svc.search(&SearchQuery {
                topic: a.topic.clone(),
                text: a.text.clone(),
                semantic: a.semantic.clone(),
                limit: a.limit,
                offset: a.offset,
                deep: a.deep,
            })?;
            Ok(render_list(&out, json))
        }
        Commands::Update(a) => {
            let e = svc.update(UpdateCommand {
                topic: a.topic.clone(),
                id: a.id.clone(),
                when_text: a.when.clone(),
                if_text: a.if_.clone(),
                do_text: a.do_.clone(),
                check_text: a.check.clone(),
            })?;
            Ok(render_identity(&e.topic, &e.id, json, "modified"))
        }
        Commands::Delete(a) => {
            if !confirm_destructive(&format!("delete ({}/{})?", a.topic, a.id), a.yes) {
                return Err(AppError::InvalidInput("delete not confirmed".to_string()));
            }
            svc.delete(&a.topic, &a.id)?;
            Ok(render_identity(&a.topic, &a.id, json, "deleted"))
        }
        Commands::Promote(a) => {
            let e = svc.promote(&FeedbackCommand {
                topic: a.topic.clone(),
                id: a.id.clone(),
            })?;
            Ok(render_feedback(&e, json, "promoted"))
        }
        Commands::Demote(a) => {
            let e = svc.demote(&FeedbackCommand {
                topic: a.topic.clone(),
                id: a.id.clone(),
            })?;
            Ok(render_feedback(&e, json, "demoted"))
        }
        Commands::Clear(a) => {
            let target = if a.all {
                "ALL topics".to_string()
            } else {
                format!("topic '{}'", a.topic.as_deref().unwrap_or("?"))
            };
            if !confirm_destructive(&format!("clear {target}?"), a.yes) {
                return Err(AppError::InvalidInput("clear not confirmed".to_string()));
            }
            let cc = if a.all {
                ClearCommand::All
            } else {
                ClearCommand::Topic(a.topic.clone().unwrap_or_default())
            };
            let summary = svc.clear(&cc)?;
            Ok(render_clear(summary, json))
        }
        Commands::Embedding(e) => {
            let commands::EmbeddingCommands::Migrate(a) = &e.command;
            if a.prune && !confirm_destructive("prune obsolete embedding vectors?", a.yes) {
                return Err(AppError::InvalidInput(
                    "embedding prune not confirmed".to_string(),
                ));
            }
            let summary = svc.migrate_embeddings(a.prune)?;
            Ok(render_embedding_migration(&summary, json))
        }
        Commands::Db(_) => Err(AppError::InvalidInput(
            "db commands are handled by the main dispatch".to_string(),
        )),
        Commands::Serve(_) | Commands::Mcp(_) => Err(AppError::InvalidInput(
            "serve/mcp are handled by the main dispatch".to_string(),
        )),
        Commands::Topic(t) => {
            let (query, level, limit, offset, deep) = match &t.command {
                commands::TopicCommands::List(a) => (None, a.level, a.limit, a.offset, a.deep),
                commands::TopicCommands::Search(a) => {
                    (Some(a.query.clone()), a.level, a.limit, a.offset, a.deep)
                }
            };
            let out = svc.list_topics(&TopicQuery {
                query,
                level,
                limit,
                offset,
                deep,
            })?;
            Ok(if json {
                serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string())
            } else if out.is_empty() {
                "(no topics)".to_string()
            } else {
                out.join("\n")
            })
        }
        Commands::Export(_) | Commands::Import(_) => Err(AppError::InvalidInput(
            "export/import run in main so stdout/file stay streamable".to_string(),
        )),
        Commands::Config(_) => Err(AppError::InvalidInput(
            "config is handled by the main dispatch".to_string(),
        )),
    }
}
