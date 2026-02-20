pub mod db;

use axum::{Json, Router, routing::get};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// アプリケーション共有状態
#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
}

/// ヘルスチェックのレスポンス
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// ヘルスチェックハンドラ
async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Axum ルーターを生成する
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;

    #[tokio::test]
    async fn health_check_returns_ok() {
        let db = db::init_test_db().await.unwrap();
        let state = AppState { db };
        let server = TestServer::new(create_router(state)).unwrap();
        let response = server.get("/health").await;

        response.assert_status_ok();
        response.assert_json(&serde_json::json!({ "status": "ok" }));
    }
}
