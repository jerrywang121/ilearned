use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Router;

use crate::application::MemoryService;
use crate::error::AppError;
use crate::storage::embeddings::VectorStore;
use crate::storage::repository::ExperienceRepo;

/// Placeholder web index (full UI in Task 7).
pub async fn web_placeholder() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "web UI arrives in Task 7")
}

/// Placeholder MCP endpoint (full tools in Task 8).
pub async fn mcp_placeholder() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "MCP arrives in Task 8")
}

/// One listener for REST + web + MCP.
pub fn build_router<R: ExperienceRepo + VectorStore + 'static>(
    svc: Arc<MemoryService<R>>,
) -> Router {
    crate::surfaces::http::rest::rest_routes::<R>()
        .merge(crate::surfaces::http::web::web_routes::<R>())
        .route(
            "/mcp",
            axum::routing::get(mcp_placeholder).post(mcp_placeholder),
        )
        .with_state(svc)
}

pub async fn serve_from_shared<R: ExperienceRepo + VectorStore + 'static>(
    svc: Arc<MemoryService<R>>,
    bind: std::net::SocketAddr,
) -> Result<(), AppError> {
    let app = build_router::<R>(svc);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}

pub async fn serve<R: ExperienceRepo + VectorStore + 'static>(
    svc: MemoryService<R>,
    bind: std::net::SocketAddr,
) -> Result<(), AppError> {
    serve_from_shared(Arc::new(svc), bind).await
}
