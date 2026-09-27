use std::sync::Arc;

use axum::Router;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery,
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
    #[schemars(description = "Filter by topic")]
    pub topic: Option<String>,
    #[schemars(description = "FTS5 full-text query")]
    pub text: Option<String>,
    #[schemars(description = "Semantic query (requires embedding provider)")]
    pub semantic: Option<String>,
    #[schemars(description = "Max results (default 20, clamped to 100)")]
    pub limit: Option<u32>,
    #[schemars(description = "Result offset (default 0)")]
    pub offset: Option<u32>,
    #[schemars(description = "Include inactive records")]
    pub deep: Option<bool>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AddArgs {
    pub topic: String,
    pub when: String,
    #[serde(rename = "if")]
    pub if_text: String,
    #[serde(rename = "do")]
    pub do_text: String,
    pub check: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ModifyArgs {
    pub topic: String,
    pub id: String,
    pub when: Option<String>,
    #[serde(rename = "if")]
    pub if_text: Option<String>,
    #[serde(rename = "do")]
    pub do_text: Option<String>,
    pub check: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct FeedbackArgs {
    pub topic: String,
    pub id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct DeleteArgs {
    pub topic: String,
    pub id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ClearArgs {
    pub topic: Option<String>,
    pub all: Option<bool>,
    #[schemars(description = "Must be true: destructive-action confirmation")]
    pub confirm: Option<bool>,
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

#[tool_router(router = tool_router, server_handler)]
impl IlearnedTools {
    #[tool(description = "Search experiences by topic, full-text, or semantic query")]
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

    #[tool(description = "Add a new experience")]
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
        ok_json(&e)
    }

    #[tool(description = "Modify an existing experience (at least one field required)")]
    fn modify(
        &self,
        Parameters(a): Parameters<ModifyArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let e = self
            .svc
            .modify(ModifyCommand {
                topic: a.topic,
                id: a.id,
                when_text: a.when,
                if_text: a.if_text,
                do_text: a.do_text,
                check_text: a.check,
            })
            .map_err(map_err)?;
        ok_json(&e)
    }

    #[tool(
        description = "Soft-delete an experience (idempotent on already-deleted; not-found if never existed)"
    )]
    fn delete(
        &self,
        Parameters(a): Parameters<DeleteArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.svc.delete(&a.topic, &a.id).map_err(map_err)?;
        ok_json(&serde_json::json!({"deleted": true}))
    }

    #[tool(description = "Promote an experience (increments good_count)")]
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
        ok_json(&e)
    }

    #[tool(description = "Downgrade an experience (increments bad_count)")]
    fn downgrade(
        &self,
        Parameters(a): Parameters<FeedbackArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let e = self
            .svc
            .downgrade(&FeedbackCommand {
                topic: a.topic,
                id: a.id,
            })
            .map_err(map_err)?;
        ok_json(&e)
    }

    #[tool(description = "Clear experiences by topic or all (requires confirm=true)")]
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
        let n = self.svc.clear(&cmd).map_err(map_err)?;
        ok_json(&serde_json::json!({"cleared": n}))
    }
}

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
