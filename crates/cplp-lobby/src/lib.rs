pub mod auth;
pub mod db;
pub mod error;
pub mod jwt;
pub mod routes;
pub mod ws;

use axum::{Json, Router, routing::get};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// アプリケーション共有状態
#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    pub oauth: auth::OAuthConfig,
    pub jwt_secret: String,
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
        // OAuth ルート
        .route("/auth/{provider}", get(auth::oauth_start))
        .route("/auth/{provider}/callback", get(auth::oauth_callback))
        .route("/auth/me", get(auth::get_me))
        // グループ & セッション ルート
        .merge(routes::groups::router())
        .merge(routes::sessions::router())
        // WebSocket
        .merge(ws::router())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;

    /// テスト用の AppState を生成するヘルパー
    async fn test_state() -> AppState {
        let db = db::init_test_db().await.unwrap();
        AppState {
            db,
            oauth: auth::OAuthConfig {
                github: None,
                google: None,
                discord: None,
            },
            jwt_secret: "test-secret-key".to_string(),
        }
    }

    #[tokio::test]
    async fn health_check_returns_ok() {
        let state = test_state().await;
        let server = TestServer::new(create_router(state)).unwrap();
        let response = server.get("/health").await;

        response.assert_status_ok();
        response.assert_json(&serde_json::json!({ "status": "ok" }));
    }

    #[tokio::test]
    async fn auth_me_requires_authentication() {
        let state = test_state().await;
        let server = TestServer::new(create_router(state)).unwrap();
        let response = server.get("/auth/me").await;

        response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_me_returns_user_id() {
        let state = test_state().await;
        let jwt_secret = state.jwt_secret.clone();
        let token = jwt::create_token("users:me123", &jwt_secret).unwrap();
        let server = TestServer::new(create_router(state)).unwrap();

        let response = server
            .get("/auth/me")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
            )
            .await;

        response.assert_status_ok();
        response.assert_json(&serde_json::json!({ "user_id": "users:me123" }));
    }
}
