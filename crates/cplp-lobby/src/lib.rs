use axum::{Json, Router, routing::get};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// アプリケーション共有状態（今後 DB 接続等を追加）
#[derive(Clone, Debug)]
pub struct AppState {}

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
pub fn create_router() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;

    #[tokio::test]
    async fn health_check_returns_ok() {
        let server = TestServer::new(create_router()).unwrap();
        let response = server.get("/health").await;

        response.assert_status_ok();
        response.assert_json(&serde_json::json!({ "status": "ok" }));
    }
}
