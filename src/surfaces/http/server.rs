use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Router;

use crate::application::MemoryService;
use crate::error::AppError;

/// Placeholder web index (full UI in Task 7).
pub async fn web_placeholder() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "web UI arrives in Task 7")
}

/// Placeholder MCP endpoint (full tools in Task 8).
#[allow(dead_code)]
pub async fn mcp_placeholder() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "MCP arrives in Task 8")
}

/// One listener for REST + web + MCP. Each sub-router gets its state up
/// front (becoming `Router<()>`), then the stateless routers merge.
pub fn build_router(svc: Arc<MemoryService<crate::storage::SqliteRepo>>) -> Router {
    let rest = crate::surfaces::http::rest::rest_routes::<crate::storage::SqliteRepo>()
        .with_state(Arc::clone(&svc));
    let web = crate::surfaces::http::web::web_routes::<crate::storage::SqliteRepo>()
        .with_state(Arc::clone(&svc));
    let mcp = crate::surfaces::http::mcp::mcp_router(svc);
    rest.merge(web).merge(mcp)
}

pub async fn serve_from_shared(
    svc: Arc<MemoryService<crate::storage::SqliteRepo>>,
    bind: std::net::SocketAddr,
) -> Result<(), AppError> {
    let app = build_router(svc);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}

pub async fn serve(
    svc: MemoryService<crate::storage::SqliteRepo>,
    bind: std::net::SocketAddr,
) -> Result<(), AppError> {
    serve_from_shared(Arc::new(svc), bind).await
}
