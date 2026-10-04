use crate::application::EmbeddingMigrationSummary;
use crate::domain::experience::Experience;
use crate::domain::ClearSummary;
use crate::error::AppError;

/// Exit-code contract: 0 ok, 1 not-found, 2 invalid, 3 embedding, 4 internal.
pub fn exit_code(e: &AppError) -> i32 {
    match e {
        AppError::NotFound { .. } => 1,
        AppError::InvalidInput(_) | AppError::InvalidFtsSyntax(_) => 2,
        AppError::EmbeddingUnavailable(_) => 3,
        AppError::Storage(_) | AppError::Internal(_) => 4,
    }
}

pub fn render_experience(e: &Experience, json: bool) -> String {
    if json {
        serde_json::to_string(e).unwrap_or_else(|_| "{}".to_string())
    } else {
        format!(
            "({}/{}) [{}] good={} bad={} updated={}\n  when: {}\n  if: {}\n  do: {}\n  check: {}",
            e.topic,
            e.id,
            state_str(e),
            e.good_count,
            e.bad_count,
            e.updated_at.to_rfc3339(),
            e.when_text,
            e.if_text,
            e.do_text,
            e.check_text
        )
    }
}

pub fn render_identity(topic: &str, id: &str, json: bool, action: &str) -> String {
    if json {
        serde_json::json!({action: {"topic": topic, "id": id}}).to_string()
    } else {
        format!("{action} ({topic}/{id})")
    }
}

pub fn render_feedback(e: &Experience, json: bool, action: &str) -> String {
    if json {
        serde_json::json!({
            "modified": {
                "topic": e.topic,
                "id": e.id,
                "good_count": e.good_count,
                "bad_count": e.bad_count,
                "state": state_str(e),
            }
        })
        .to_string()
    } else if matches!(e.state, crate::domain::State::Deleted) {
        format!(
            "{action} ({}/{}) good={} bad={} [auto-deleted]",
            e.topic, e.id, e.good_count, e.bad_count
        )
    } else {
        format!(
            "{action} ({}/{}) good={} bad={}",
            e.topic, e.id, e.good_count, e.bad_count
        )
    }
}

pub fn render_clear(summary: ClearSummary, json: bool) -> String {
    if json {
        serde_json::json!({
            "cleared": {
                "num_of_topics": summary.topics,
                "num_of_items": summary.items,
            }
        })
        .to_string()
    } else {
        format!(
            "cleared {} topic(s), {} experience(s)",
            summary.topics, summary.items
        )
    }
}

/// Render the result of rebuilding derived embedding vectors.
pub fn render_embedding_migration(summary: &EmbeddingMigrationSummary, json: bool) -> String {
    if json {
        serde_json::json!({
            "embedding_migration": {
                "model": summary.model,
                "dims": summary.dims,
                "total": summary.total,
                "migrated": summary.migrated,
                "pruned": summary.pruned,
            }
        })
        .to_string()
    } else {
        let dims = summary
            .dims
            .map_or_else(|| "unknown".to_string(), |dims| dims.to_string());
        format!(
            "migrated {} of {} experience(s), model={}, dims={}, pruned {} vector(s)",
            summary.migrated, summary.total, summary.model, dims, summary.pruned
        )
    }
}

pub fn render_list(list: &[Experience], json: bool) -> String {
    if json {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
    } else if list.is_empty() {
        "(no experiences)".to_string()
    } else {
        list.iter()
            .map(|e| {
                format!(
                    "({}/{}) [{}] {}",
                    e.topic,
                    e.id,
                    state_str(e),
                    e.when_text
                        .chars()
                        .take(80)
                        .collect::<String>()
                        .replace('\n', " ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn render_error(e: &AppError, json: bool) -> String {
    if json {
        serde_json::json!({"error": e.to_string()}).to_string()
    } else {
        format!("error: {e}")
    }
}

fn state_str(e: &Experience) -> &'static str {
    match e.state {
        crate::domain::experience::State::Active => "active",
        crate::domain::experience::State::Inactive => "inactive",
        crate::domain::experience::State::Deleted => "deleted",
        crate::domain::experience::State::Forgotten => "forgotten",
    }
}

/// Prompt `y/N` on stderr; returns true only for y/yes. `yes_flag`
/// bypasses the prompt (for `--yes`).
pub fn confirm_destructive(action: &str, yes_flag: bool) -> bool {
    if yes_flag {
        return true;
    }
    use std::io::{BufRead, Write};
    let _ = write!(std::io::stderr(), "{action} [y/N]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let n = std::io::BufReader::new(std::io::stdin())
        .read_line(&mut line)
        .unwrap_or(0);
    if n == 0 {
        return false; // EOF / non-interactive without --yes => refuse
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}
