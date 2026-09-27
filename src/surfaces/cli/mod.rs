pub mod commands;
pub mod render;

pub use commands::{Cli, Commands};
pub use render::{confirm_destructive, exit_code, render_error, render_experience, render_list};

use crate::application::MemoryService;
use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery,
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
            Ok(render_experience(&e, json))
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
        Commands::Modify(a) => {
            let e = svc.modify(ModifyCommand {
                topic: a.topic.clone(),
                id: a.id.clone(),
                when_text: a.when.clone(),
                if_text: a.if_.clone(),
                do_text: a.do_.clone(),
                check_text: a.check.clone(),
            })?;
            Ok(render_experience(&e, json))
        }
        Commands::Delete(a) => {
            if !confirm_destructive(&format!("delete ({}/{})?", a.topic, a.id), a.yes) {
                return Err(AppError::InvalidInput("delete not confirmed".to_string()));
            }
            svc.delete(&a.topic, &a.id)?;
            Ok(if json {
                serde_json::json!({"deleted": true}).to_string()
            } else {
                format!("deleted ({}/{})", a.topic, a.id)
            })
        }
        Commands::Promote(a) => {
            let e = svc.promote(&FeedbackCommand {
                topic: a.topic.clone(),
                id: a.id.clone(),
            })?;
            Ok(render_experience(&e, json))
        }
        Commands::Downgrade(a) => {
            let e = svc.downgrade(&FeedbackCommand {
                topic: a.topic.clone(),
                id: a.id.clone(),
            })?;
            Ok(render_experience(&e, json))
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
            let n = svc.clear(&cc)?;
            Ok(if json {
                serde_json::json!({"cleared": n}).to_string()
            } else {
                format!("cleared {n} experience(s)")
            })
        }
        Commands::Serve(_) | Commands::Mcp => Err(AppError::InvalidInput(
            "serve/mcp are handled by the main dispatch".to_string(),
        )),
    }
}
