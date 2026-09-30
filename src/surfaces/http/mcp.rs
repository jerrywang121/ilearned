use std::sync::Arc;

use axum::Router;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServerHandler;
use rmcp::ServiceExt;
use rmcp::{tool, tool_handler, tool_router};
use serde::Deserialize;

use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, SearchQuery, TopicQuery, UpdateCommand,
};
use crate::domain::experience::Experience;
use crate::error::AppError;
use crate::surfaces::http::rest::Shared;

fn map_err(e: AppError) -> rmcp::ErrorData {
    match e {
        AppError::InvalidInput(m) | AppError::InvalidFtsSyntax(m) => {
            rmcp::ErrorData::invalid_params(m, None)
        }
        AppError::NotFound { topic, id } => rmcp::ErrorData::resource_not_found(
            format!("experience not found: ({topic}, {id})"),
            None,
        ),
        AppError::EmbeddingUnavailable(m) => rmcp::ErrorData::internal_error(
            format!("embedding unavailable (503-equivalent): {m}"),
            None,
        ),
        AppError::Storage(m) | AppError::Internal(m) => rmcp::ErrorData::internal_error(m, None),
    }
}

fn ok_json<T: serde::Serialize>(v: &T) -> Result<CallToolResult, rmcp::ErrorData> {
    ContentBlock::json(v)
        .map(|c| CallToolResult::success(vec![c]))
        .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))
}

// --- Tool argument schemas: mirror the domain commands exactly ---

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchArgs {
    #[schemars(description = "Filter by topic (compound key with id)")]
    pub topic: Option<String>,
    #[schemars(description = "FTS5 full-text query over when/if/do/check text")]
    pub text: Option<String>,
    #[schemars(description = "Semantic query (requires embedding provider; errors when unset)")]
    pub semantic: Option<String>,
    #[schemars(description = "Max results (default 20, clamped to 100)")]
    pub limit: Option<u32>,
    #[schemars(description = "Result offset for pagination (default 0)")]
    pub offset: Option<u32>,
    #[schemars(description = "Include inactive records (deleted/forgotten always excluded)")]
    pub deep: Option<bool>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AddArgs {
    #[schemars(
        description = "Hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; search accepts # multi-level wildcard"
    )]
    pub topic: String,
    #[schemars(
        description = "Scenario this experience applies to, including context, conditions, and constraints"
    )]
    pub when: String,
    #[serde(rename = "if")]
    #[schemars(description = "Trigger(s), e.g. something happened, observed, or detected")]
    pub if_text: String,
    #[serde(rename = "do")]
    #[schemars(
        description = "Action(s) the agent should take / try, include steps, procedures, and instructions"
    )]
    pub do_text: String,
    #[schemars(
        description = "Signal(s) to verify the experience was useful, list what to look for to confirm the scenario matches, how to identify triggers, and what can be used to confirm the results of the action"
    )]
    pub check: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct UpdateArgs {
    #[schemars(description = "Topic of the record to update")]
    pub topic: String,
    #[schemars(description = "Id of the record to update")]
    pub id: String,
    #[schemars(description = "New scenario text (blank values ignored)")]
    pub when: Option<String>,
    #[serde(rename = "if")]
    #[schemars(description = "New trigger text (blank values ignored)")]
    pub if_text: Option<String>,
    #[serde(rename = "do")]
    #[schemars(description = "New action text (blank values ignored)")]
    pub do_text: Option<String>,
    #[schemars(description = "New check text (blank values ignored)")]
    pub check: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct FeedbackArgs {
    #[schemars(description = "Topic of the record")]
    pub topic: String,
    #[schemars(description = "Id of the record")]
    pub id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct DeleteArgs {
    #[schemars(description = "Topic of the record to delete")]
    pub topic: String,
    #[schemars(description = "Id of the record to delete")]
    pub id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ClearArgs {
    #[schemars(description = "Clear a single topic (exactly one of topic/all required)")]
    pub topic: Option<String>,
    #[schemars(description = "Clear all topics when true")]
    pub all: Option<bool>,
    #[schemars(description = "Must be true: destructive-action confirmation")]
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct TopicsListArgs {
    #[schemars(description = "Limit hierarchy depth (applied after matching)")]
    pub level: Option<u32>,
    #[schemars(description = "Max topics (default 20, clamped to 100)")]
    pub limit: Option<u32>,
    #[schemars(description = "Result offset for pagination (default 0)")]
    pub offset: Option<u32>,
    #[schemars(description = "Include topics that only have inactive records")]
    pub deep: Option<bool>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct TopicsSearchArgs {
    #[schemars(
        description = "Substring or # multi-level wildcard pattern (e.g. hotel, travel/#, #/checkout)"
    )]
    pub query: String,
    #[schemars(description = "Limit hierarchy depth (applied after matching)")]
    pub level: Option<u32>,
    #[schemars(description = "Max topics (default 20, clamped to 100)")]
    pub limit: Option<u32>,
    #[schemars(description = "Result offset for pagination (default 0)")]
    pub offset: Option<u32>,
    #[schemars(description = "Include topics that only have inactive records")]
    pub deep: Option<bool>,
}

pub struct IlearnedTools {
    svc: Shared<crate::storage::SqliteRepo>,
    #[allow(dead_code)]
    router: ToolRouter<Self>,
}

impl IlearnedTools {
    pub fn new(svc: Shared<crate::storage::SqliteRepo>) -> Self {
        Self {
            svc,
            router: Self::tool_router(),
        }
    }
}

impl Clone for IlearnedTools {
    fn clone(&self) -> Self {
        Self {
            svc: Arc::clone(&self.svc),
            router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl IlearnedTools {
    #[tool(
        description = "Search experiences by topic, full-text, or semantic query (deleted/forgotten excluded; inactive only with deep=true)"
    )]
    fn search(
        &self,
        Parameters(a): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let out: Vec<Experience> = self
            .svc
            .search(&SearchQuery {
                topic: a.topic,
                text: a.text,
                semantic: a.semantic,
                limit: a.limit.unwrap_or(20),
                offset: a.offset.unwrap_or(0),
                deep: a.deep.unwrap_or(false),
            })
            .map_err(map_err)?;
        ok_json(&out)
    }

    #[tool(description = "Add a new experience (sets good_count=1, state=active)")]
    fn add(&self, Parameters(a): Parameters<AddArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        let e = self
            .svc
            .add(AddCommand {
                topic: a.topic,
                when_text: a.when,
                if_text: a.if_text,
                do_text: a.do_text,
                check_text: a.check,
            })
            .map_err(map_err)?;
        ok_json(&serde_json::json!({
            "added": {"topic": e.topic, "id": e.id}
        }))
    }

    #[tool(
        description = "Update an existing experience (at least one non-blank field required; blank values ignored)"
    )]
    fn update(
        &self,
        Parameters(a): Parameters<UpdateArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let e = self
            .svc
            .update(UpdateCommand {
                topic: a.topic,
                id: a.id,
                when_text: a.when,
                if_text: a.if_text,
                do_text: a.do_text,
                check_text: a.check,
            })
            .map_err(map_err)?;
        ok_json(&serde_json::json!({
            "modified": {"topic": e.topic, "id": e.id}
        }))
    }

    #[tool(
        description = "Soft-delete an experience (idempotent on already-deleted; not-found if never existed)"
    )]
    fn delete(
        &self,
        Parameters(a): Parameters<DeleteArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.svc.delete(&a.topic, &a.id).map_err(map_err)?;
        ok_json(&serde_json::json!({
            "deleted": {"topic": a.topic, "id": a.id}
        }))
    }

    #[tool(description = "Promote an experience (increments good_count, restores to active)")]
    fn promote(
        &self,
        Parameters(a): Parameters<FeedbackArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let e = self
            .svc
            .promote(&FeedbackCommand {
                topic: a.topic,
                id: a.id,
            })
            .map_err(map_err)?;
        ok_json(&serde_json::json!({
            "modified": {
                "topic": e.topic,
                "id": e.id,
                "good_count": e.good_count,
                "bad_count": e.bad_count,
                "state": e.state_str(),
            }
        }))
    }

    #[tool(
        description = "Demote an experience (increments bad_count; auto-deletes when the good ratio drops below the configured threshold)"
    )]
    fn demote(
        &self,
        Parameters(a): Parameters<FeedbackArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let e = self
            .svc
            .demote(&FeedbackCommand {
                topic: a.topic,
                id: a.id,
            })
            .map_err(map_err)?;
        ok_json(&serde_json::json!({
            "modified": {
                "topic": e.topic,
                "id": e.id,
                "good_count": e.good_count,
                "bad_count": e.bad_count,
                "state": e.state_str(),
            }
        }))
    }

    #[tool(
        description = "Clear experiences by topic or all (destructive; requires confirm=true plus exactly one of topic/all)"
    )]
    fn clear(
        &self,
        Parameters(a): Parameters<ClearArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if a.confirm != Some(true) {
            return Err(rmcp::ErrorData::invalid_params(
                "clear requires confirm=true",
                None,
            ));
        }
        let cmd = match (a.topic, a.all.unwrap_or(false)) {
            (Some(t), _) => ClearCommand::Topic(t),
            (None, true) => ClearCommand::All,
            _ => {
                return Err(rmcp::ErrorData::invalid_params(
                    "clear requires topic or all=true",
                    None,
                ));
            }
        };
        let summary = self.svc.clear(&cmd).map_err(map_err)?;
        ok_json(&serde_json::json!({
            "cleared": {
                "num_of_topics": summary.topics,
                "num_of_items": summary.items,
            }
        }))
    }

    #[tool(description = "List existing topics, optionally truncated to a hierarchy depth")]
    fn topics_list(
        &self,
        Parameters(a): Parameters<TopicsListArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let out = self
            .svc
            .list_topics(&TopicQuery {
                query: None,
                level: a.level,
                limit: a.limit.unwrap_or(20),
                offset: a.offset.unwrap_or(0),
                deep: a.deep.unwrap_or(false),
            })
            .map_err(map_err)?;
        ok_json(&out)
    }

    #[tool(
        description = "Search topics by substring or # multi-level wildcard pattern (e.g. hotel, travel/#, #/checkout)"
    )]
    fn topics_search(
        &self,
        Parameters(a): Parameters<TopicsSearchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let out = self
            .svc
            .list_topics(&TopicQuery {
                query: Some(a.query),
                level: a.level,
                limit: a.limit.unwrap_or(20),
                offset: a.offset.unwrap_or(0),
                deep: a.deep.unwrap_or(false),
            })
            .map_err(map_err)?;
        ok_json(&out)
    }
}

#[tool_handler(
    name = "ilearned",
    instructions = "Local-first AI agent memory: a live rule book of learned experiences (when/if/do/check) grouped by hierarchical topic (e.g. travel/hotel/checkout; search accepts # multi-level wildcard). Use `search` to find applicable rules, `add` to record new lessons, `update` to refine them, `promote`/`demote` for feedback, `delete`/`clear` for removal (destructive actions need `confirm=true`), `topics_list`/`topics_search` to browse topics."
)]
impl ServerHandler for IlearnedTools {}

pub fn mcp_service(
    svc: Shared<crate::storage::SqliteRepo>,
) -> StreamableHttpService<IlearnedTools, LocalSessionManager> {
    let factory_svc = Arc::clone(&svc);
    StreamableHttpService::new(
        move || Ok(IlearnedTools::new(Arc::clone(&factory_svc))),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

/// Router carrying a stateful MCP service: axum `nest_service` cannot inject
/// state, so we build the full `/mcp` route from the shared service here.
/// The returned router is stateless (no `Shared` state) so it can merge with
/// the stateful REST/web routers after they take state.
pub fn mcp_router(svc: Shared<crate::storage::SqliteRepo>) -> Router<()> {
    let service = mcp_service(svc);
    Router::new().nest_service("/mcp", service)
}

/// Serve MCP over stdio (stdin/stdout) for harness use. Stdout stays pure
/// JSON-RPC: all logging/errors go to stderr, never stdout.
pub async fn serve_stdio(
    svc: Arc<crate::application::MemoryService<crate::storage::SqliteRepo>>,
) -> Result<(), crate::error::AppError> {
    let running = IlearnedTools::new(svc)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;
    Ok(())
}
