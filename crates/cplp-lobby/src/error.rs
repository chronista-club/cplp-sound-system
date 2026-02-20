//! アプリケーションエラー型
//!
//! Axum ハンドラから `Result<T, AppError>` を返すための共通エラー型。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Axum ハンドラ用のエラーラッパー
///
/// `anyhow::Error` をラップし、500 JSON レスポンスに変換する。
pub struct AppError(pub anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.0.to_string(),
        });
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError(err.into())
    }
}
