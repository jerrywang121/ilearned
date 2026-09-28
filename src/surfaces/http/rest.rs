use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use serde::{Deserialize, Serialize};

use crate::application::MemoryService;
use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery, TopicQuery,
};
use crate::error::AppError;
use crate::storage::embeddings::VectorStore;
use crate::storage::repository::ExperienceRepo;

pub type Shared<R> = Arc<MemoryService<R>>;

pub struct ApiError(AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match &self.0 {
            AppError::InvalidInput(m) | AppError::InvalidFtsSyntax(m) => {
                (StatusCode::BAD_REQUEST, m.clone())
            }
            AppError::NotFound { topic, id } => (
                StatusCode::NOT_FOUND,
                format!("experience not found: ({topic}, {id})"),
            ),
            AppError::EmbeddingUnavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m.clone()),
            AppError::Storage(m) | AppError::Internal(m) => {
                (StatusCode::INTERNAL_SERVER_ERROR, m.clone())
            }
        };
        (status, Json(serde_json::json!({"error": msg}))).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct AddRequest {
    pub topic: String,
    #[serde(rename = "when")]
    pub when_text: Option<String>,
    #[serde(rename = "if")]
    pub if_text: Option<String>,
    #[serde(rename = "do")]
    pub do_text: Option<String>,
    pub check: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ModifyRequest {
    #[serde(rename = "when")]
    pub when_text: Option<String>,
    #[serde(rename = "if")]
    pub if_text: Option<String>,
    #[serde(rename = "do")]
    pub do_text: Option<String>,
    pub check: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub topic: Option<String>,
    pub text: Option<String>,
    pub semantic: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub deep: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ClearParams {
    pub topic: Option<String>,
    pub all: Option<bool>,
    pub confirm: Option<bool>,
}

/// Topic list/search params: no `q` = list, with `q` = search
/// (substring or `#` multi-level wildcard pattern).
#[derive(Debug, Deserialize)]
pub struct TopicParams {
    pub level: Option<u32>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub deep: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Health {
    pub ok: bool,
}

/// Readiness probe: verifies SQLite reachable + migrations applied.
/// Returns 200 `{"ok":true}`.
pub async fn healthz<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
) -> Result<Json<Health>, ApiError> {
    // Verify SQLite reachable + migrations applied: browse must work.
    svc.search(&SearchQuery {
        limit: 0,
        ..Default::default()
    })?;
    Ok(Json(Health { ok: true }))
}

/// Search/browse experiences.
/// Query params: `topic` filter, `text` FTS5 query, `semantic` vector query
/// (needs provider, else 503), `limit` (default 20, clamped to 100),
/// `offset` (default 0), `deep` includes inactive (deleted/forgotten never shown).
pub async fn search<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Query(p): Query<SearchParams>,
) -> Result<Json<Vec<crate::domain::experience::Experience>>, ApiError> {
    let out = svc.search(&SearchQuery {
        topic: p.topic,
        text: p.text,
        semantic: p.semantic,
        limit: p.limit.unwrap_or(20),
        offset: p.offset.unwrap_or(0),
        deep: p.deep.unwrap_or(false),
    })?;
    Ok(Json(out))
}

/// Add a new experience (body: topic/when/if/do/check, all required, non-blank).
/// Returns 201 with the created record (good_count=1, state=active).
pub async fn add<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Json(body): Json<AddRequest>,
) -> Result<(StatusCode, Json<crate::domain::experience::Experience>), ApiError> {
    let some = |o: Option<String>, name: &str| {
        o.filter(|s| !s.trim().is_empty())
            .ok_or_else(|| AppError::InvalidInput(format!("{name} is required")))
    };
    let cmd = AddCommand {
        topic: some(Some(body.topic), "topic")?,
        when_text: some(body.when_text, "when")?,
        if_text: some(body.if_text, "if")?,
        do_text: some(body.do_text, "do")?,
        check_text: some(body.check, "check")?,
    };
    let e = svc.add(cmd)?;
    Ok((StatusCode::CREATED, Json(e)))
}

/// Modify selected fields of an experience (body: non-blank subset of
/// when/if/do/check; all-blank is 400). Missing/deleted id is 404.
pub async fn modify<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
    Json(body): Json<ModifyRequest>,
) -> Result<Json<crate::domain::experience::Experience>, ApiError> {
    let e = svc.modify(ModifyCommand {
        topic,
        id,
        when_text: body.when_text,
        if_text: body.if_text,
        do_text: body.do_text,
        check_text: body.check,
    })?;
    Ok(Json(e))
}

/// Soft-delete one experience (204, no body). Never-existing id is 404;
/// already-deleted is idempotent 204.
pub async fn delete_one<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    // Delete on a never-existing id returns NotFound (404); deleting an
    // already-deleted record is idempotent (204).
    svc.delete(&topic, &id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Positive feedback: increments good_count, restores record to active.
pub async fn promote<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<Json<crate::domain::experience::Experience>, ApiError> {
    Ok(Json(svc.promote(&FeedbackCommand { topic, id })?))
}

/// Negative feedback: increments bad_count.
pub async fn downgrade<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<Json<crate::domain::experience::Experience>, ApiError> {
    Ok(Json(svc.downgrade(&FeedbackCommand { topic, id })?))
}

/// Clear by topic or all (destructive): requires `?confirm=true` plus exactly
/// one of `?topic=X` / `?all=true`. Returns unique topic and item counts.
pub async fn clear<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Query(p): Query<ClearParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if p.confirm != Some(true) {
        return Err(AppError::InvalidInput("clear requires confirm=true".to_string()).into());
    }
    let cmd = match (p.topic, p.all.unwrap_or(false)) {
        (Some(t), false) => ClearCommand::Topic(t),
        (None, true) => ClearCommand::All,
        _ => {
            return Err(AppError::InvalidInput(
                "clear requires exactly one of ?topic= or ?all=true".to_string(),
            )
            .into());
        }
    };
    let summary = svc.clear(&cmd)?;
    Ok(Json(serde_json::json!({
        "cleared": {
            "num_of_topics": summary.topics,
            "num_of_items": summary.items,
        }
    })))
}

/// List/search distinct topics. Query params: `level` truncates hierarchy
/// depth, `q` searches (substring or `#` pattern), `limit` (default 20,
/// clamped to 100), `offset` (default 0), `deep` includes topics that only
/// have inactive records. Returns 200 `["travel/hotel", ...]` sorted.
pub async fn topics<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Query(p): Query<TopicParams>,
) -> Result<Json<Vec<String>>, ApiError> {
    let out = svc.list_topics(&TopicQuery {
        query: p.q,
        level: p.level,
        limit: p.limit.unwrap_or(20),
        offset: p.offset.unwrap_or(0),
        deep: p.deep.unwrap_or(false),
    })?;
    Ok(Json(out))
}

pub fn rest_routes<R: ExperienceRepo + VectorStore + 'static>() -> Router<Shared<R>> {
    Router::new()
        .route("/healthz", get(healthz::<R>))
        .route("/api/v1/experiences", get(search::<R>).post(add::<R>))
        .route("/api/v1/topics", get(topics::<R>))
        .route(
            "/api/v1/experiences/:topic/:id",
            patch(modify::<R>).delete(delete_one::<R>),
        )
        .route("/api/v1/experiences/:topic/:id/promote", post(promote::<R>))
        .route(
            "/api/v1/experiences/:topic/:id/downgrade",
            post(downgrade::<R>),
        )
        .route("/api/v1/experiences", delete(clear::<R>))
}

pub async fn serve(
    svc: MemoryService<crate::storage::SqliteRepo>,
    bind: SocketAddr,
) -> Result<(), AppError> {
    let app = crate::surfaces::http::server::build_router(Arc::new(svc));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}
