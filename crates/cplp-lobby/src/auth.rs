//! OAuth 認証フロー + AuthUser エクストラクタ
//!
//! REQ-LOBBY-002: GitHub / Google / Discord OAuth 認証
//!
//! - `init_oauth_config()`: 環境変数から OAuth クライアントを初期化
//! - `oauth_start`: 認可 URL へリダイレクト
//! - `oauth_callback`: コールバック処理（コード交換 → ユーザー情報取得 → JWT 発行）
//! - `get_me`: 認証済みユーザー情報を取得
//! - `AuthUser`: Bearer トークンからユーザーIDを抽出するエクストラクタ

use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet, RedirectUrl, Scope,
    TokenUrl,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::jwt;

// ---------------------------------------------------------------------------
// 型定義
// ---------------------------------------------------------------------------

/// OAuth クライアント（auth_url + token_url が設定済み）の具体的な型
type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// OAuth プロバイダー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Github,
    Google,
    Discord,
}

impl OAuthProvider {
    /// 文字列からプロバイダーを解決する
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "github" => Some(Self::Github),
            "google" => Some(Self::Google),
            "discord" => Some(Self::Discord),
            _ => None,
        }
    }

    /// プロバイダー名を返す
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Google => "google",
            Self::Discord => "discord",
        }
    }
}

/// OAuth の設定（各プロバイダーは Optional）
#[derive(Clone)]
pub struct OAuthConfig {
    pub github: Option<ConfiguredClient>,
    pub google: Option<ConfiguredClient>,
    pub discord: Option<ConfiguredClient>,
}

impl OAuthConfig {
    /// プロバイダーに対応するクライアントを返す
    pub fn client_for(&self, provider: OAuthProvider) -> Option<&ConfiguredClient> {
        match provider {
            OAuthProvider::Github => self.github.as_ref(),
            OAuthProvider::Google => self.google.as_ref(),
            OAuthProvider::Discord => self.discord.as_ref(),
        }
    }
}

/// ユーザー情報（OAuth プロバイダーから取得）
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthUserInfo {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
}

/// コールバックのクエリパラメータ
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    #[allow(dead_code)]
    pub state: Option<String>,
}

/// /auth/me レスポンス
#[derive(Debug, Serialize, Deserialize)]
pub struct MeResponse {
    pub user_id: String,
}

// ---------------------------------------------------------------------------
// 認証済みユーザーエクストラクタ
// ---------------------------------------------------------------------------

/// 認証済みユーザーの ID
///
/// `Authorization: Bearer <jwt>` ヘッダーから JWT を検証し、
/// ユーザー ID を抽出する Axum エクストラクタ。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
}

impl FromRequestParts<crate::AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        // Authorization ヘッダーを取得
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "missing authorization header"})),
                )
                    .into_response()
            })?;

        // "Bearer " プレフィックスを除去
        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid authorization scheme"})),
            )
                .into_response()
        })?;

        // JWT を検証
        let claims = jwt::verify_token(token, &state.jwt_secret).map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid or expired token"})),
            )
                .into_response()
        })?;

        Ok(AuthUser {
            user_id: claims.sub,
        })
    }
}

// ---------------------------------------------------------------------------
// OAuth 設定初期化
// ---------------------------------------------------------------------------

/// 環境変数から OAuth クライアントを初期化する
///
/// 各プロバイダーの `CLIENT_ID` / `CLIENT_SECRET` が設定されていない場合、
/// そのプロバイダーは `None`（無効）になる。
pub fn init_oauth_config(base_url: &str) -> OAuthConfig {
    OAuthConfig {
        github: build_github_client(base_url),
        google: build_google_client(base_url),
        discord: build_discord_client(base_url),
    }
}

fn build_github_client(base_url: &str) -> Option<ConfiguredClient> {
    let client_id = std::env::var("GITHUB_CLIENT_ID").ok()?;
    let client_secret = std::env::var("GITHUB_CLIENT_SECRET").ok()?;

    Some(
        BasicClient::new(ClientId::new(client_id))
            .set_client_secret(ClientSecret::new(client_secret))
            .set_auth_uri(
                AuthUrl::new("https://github.com/login/oauth/authorize".to_string()).unwrap(),
            )
            .set_token_uri(
                TokenUrl::new("https://github.com/login/oauth/access_token".to_string()).unwrap(),
            )
            .set_redirect_uri(
                RedirectUrl::new(format!("{base_url}/auth/github/callback")).unwrap(),
            ),
    )
}

fn build_google_client(base_url: &str) -> Option<ConfiguredClient> {
    let client_id = std::env::var("GOOGLE_CLIENT_ID").ok()?;
    let client_secret = std::env::var("GOOGLE_CLIENT_SECRET").ok()?;

    Some(
        BasicClient::new(ClientId::new(client_id))
            .set_client_secret(ClientSecret::new(client_secret))
            .set_auth_uri(
                AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string()).unwrap(),
            )
            .set_token_uri(
                TokenUrl::new("https://oauth2.googleapis.com/token".to_string()).unwrap(),
            )
            .set_redirect_uri(
                RedirectUrl::new(format!("{base_url}/auth/google/callback")).unwrap(),
            ),
    )
}

fn build_discord_client(base_url: &str) -> Option<ConfiguredClient> {
    let client_id = std::env::var("DISCORD_CLIENT_ID").ok()?;
    let client_secret = std::env::var("DISCORD_CLIENT_SECRET").ok()?;

    Some(
        BasicClient::new(ClientId::new(client_id))
            .set_client_secret(ClientSecret::new(client_secret))
            .set_auth_uri(
                AuthUrl::new("https://discord.com/api/oauth2/authorize".to_string()).unwrap(),
            )
            .set_token_uri(
                TokenUrl::new("https://discord.com/api/oauth2/token".to_string()).unwrap(),
            )
            .set_redirect_uri(
                RedirectUrl::new(format!("{base_url}/auth/discord/callback")).unwrap(),
            ),
    )
}

// ---------------------------------------------------------------------------
// ルートハンドラ
// ---------------------------------------------------------------------------

/// GET /auth/:provider - OAuth 認可 URL へリダイレクト
pub async fn oauth_start(
    State(state): State<crate::AppState>,
    Path(provider): Path<String>,
) -> Result<Redirect, AppError> {
    let provider_enum = OAuthProvider::from_str(&provider)
        .ok_or_else(|| anyhow::anyhow!("unknown provider: {}", provider))?;

    let client = state
        .oauth
        .client_for(provider_enum)
        .ok_or_else(|| anyhow::anyhow!("provider {} is not configured", provider))?;

    let scopes = match provider_enum {
        OAuthProvider::Github => vec!["read:user", "user:email"],
        OAuthProvider::Google => vec!["openid", "email", "profile"],
        OAuthProvider::Discord => vec!["identify", "email"],
    };

    let mut auth_request = client.authorize_url(CsrfToken::new_random);
    for scope in scopes {
        auth_request = auth_request.add_scope(Scope::new(scope.to_string()));
    }
    let (auth_url, _csrf_token) = auth_request.url();

    Ok(Redirect::temporary(auth_url.as_str()))
}

/// GET /auth/:provider/callback - OAuth コールバック処理
///
/// 現時点では実際のトークン交換は行わず、構造のみを実装。
/// 本番では以下のフローになる:
/// 1. code をアクセストークンに交換
/// 2. アクセストークンでユーザー情報を取得
/// 3. DB に upsert
/// 4. JWT を発行して返す
pub async fn oauth_callback(
    State(state): State<crate::AppState>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let provider_enum = OAuthProvider::from_str(&provider)
        .ok_or_else(|| anyhow::anyhow!("unknown provider: {}", provider))?;

    let _client = state
        .oauth
        .client_for(provider_enum)
        .ok_or_else(|| anyhow::anyhow!("provider {} is not configured", provider))?;

    // TODO: 実際のトークン交換 + ユーザー情報取得を実装
    // 現在はコールバックの受信確認のみ
    tracing::info!(
        provider = provider,
        code_len = query.code.len(),
        "OAuth callback received"
    );

    // スタブ: ユーザー情報（本番では OAuth プロバイダーから取得する）
    let user_info = OAuthUserInfo {
        provider: provider_enum.as_str().to_string(),
        id: format!("stub-{}", uuid::Uuid::new_v4()),
        name: "OAuth User".to_string(),
        email: "user@example.com".to_string(),
        avatar_url: None,
    };

    // DB に upsert（oauth_provider + oauth_id でユニーク）
    let user_id = upsert_user(&state.db, &user_info).await?;

    // JWT 発行
    let token = jwt::create_token(&user_id, &state.jwt_secret)?;

    Ok(Json(serde_json::json!({
        "token": token,
        "user_id": user_id,
    })))
}

/// GET /auth/me - 認証済みユーザー情報を返す
pub async fn get_me(auth_user: AuthUser) -> Json<MeResponse> {
    Json(MeResponse {
        user_id: auth_user.user_id,
    })
}

// ---------------------------------------------------------------------------
// DB 操作
// ---------------------------------------------------------------------------

/// OAuth ユーザーを DB に upsert し、レコード ID を返す
async fn upsert_user(db: &crate::db::Db, info: &OAuthUserInfo) -> anyhow::Result<String> {
    let provider = info.provider.clone();
    let oauth_id = info.id.clone();
    let name = info.name.clone();
    let email = info.email.clone();
    let avatar_url = info.avatar_url.clone();

    // oauth_provider + oauth_id で既存ユーザーを検索
    let mut result = db
        .query("SELECT id FROM users WHERE oauth_provider = $provider AND oauth_id = $oauth_id")
        .bind(("provider", provider.clone()))
        .bind(("oauth_id", oauth_id.clone()))
        .await?;

    let existing: Vec<serde_json::Value> = result.take(0)?;

    if let Some(user) = existing.first() {
        // 既存ユーザーがいる場合はそのIDを返す
        let id = user["id"].as_str().unwrap_or_default();
        Ok(id.to_string())
    } else {
        // 新規ユーザーを作成
        let mut result = db
            .query(
                "CREATE users SET \
                 name = $name, \
                 email = $email, \
                 avatar_url = $avatar_url, \
                 oauth_provider = $provider, \
                 oauth_id = $oauth_id",
            )
            .bind(("name", name))
            .bind(("email", email))
            .bind(("avatar_url", avatar_url))
            .bind(("provider", provider))
            .bind(("oauth_id", oauth_id))
            .await?;

        let created: Vec<serde_json::Value> = result.take(0)?;
        let user = created
            .first()
            .ok_or_else(|| anyhow::anyhow!("failed to create user"))?;
        let id = user["id"].as_str().unwrap_or_default();
        Ok(id.to_string())
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_config_disabled_when_no_env() {
        // 環境変数が未設定の場合、全プロバイダーが None になる
        let config = init_oauth_config("http://localhost:3000");

        // CI 環境では環境変数が設定されていない前提
        if std::env::var("GITHUB_CLIENT_ID").is_err() {
            assert!(
                config.github.is_none(),
                "GitHub should be None without env vars"
            );
        }
        if std::env::var("GOOGLE_CLIENT_ID").is_err() {
            assert!(
                config.google.is_none(),
                "Google should be None without env vars"
            );
        }
        if std::env::var("DISCORD_CLIENT_ID").is_err() {
            assert!(
                config.discord.is_none(),
                "Discord should be None without env vars"
            );
        }
    }

    #[test]
    fn test_oauth_provider_from_str() {
        assert_eq!(
            OAuthProvider::from_str("github"),
            Some(OAuthProvider::Github)
        );
        assert_eq!(
            OAuthProvider::from_str("google"),
            Some(OAuthProvider::Google)
        );
        assert_eq!(
            OAuthProvider::from_str("discord"),
            Some(OAuthProvider::Discord)
        );
        assert_eq!(OAuthProvider::from_str("unknown"), None);
    }

    #[test]
    fn test_oauth_provider_as_str() {
        assert_eq!(OAuthProvider::Github.as_str(), "github");
        assert_eq!(OAuthProvider::Google.as_str(), "google");
        assert_eq!(OAuthProvider::Discord.as_str(), "discord");
    }

    #[tokio::test]
    async fn test_auth_user_extraction_with_valid_token() {
        use axum::routing::get;
        use axum::Router;

        let jwt_secret = "test-secret-for-auth-user";
        let user_id = "users:test123";

        // テスト用トークン発行
        let token =
            jwt::create_token(user_id, jwt_secret).expect("token creation should succeed");

        // テスト用ルーターを構築
        let db = crate::db::init_test_db().await.unwrap();
        let state = crate::AppState {
            db,
            oauth: OAuthConfig {
                github: None,
                google: None,
                discord: None,
            },
            jwt_secret: jwt_secret.to_string(),
        };

        async fn handler(auth_user: AuthUser) -> Json<MeResponse> {
            Json(MeResponse {
                user_id: auth_user.user_id,
            })
        }

        let app = Router::new()
            .route("/test", get(handler))
            .with_state(state);

        let server = axum_test::TestServer::new(app).unwrap();
        let res = server
            .get("/test")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
            )
            .await;

        res.assert_status_ok();
        let body: MeResponse = res.json();
        assert_eq!(body.user_id, user_id);
    }

    #[tokio::test]
    async fn test_auth_user_extraction_without_token() {
        use axum::routing::get;
        use axum::Router;

        let db = crate::db::init_test_db().await.unwrap();
        let state = crate::AppState {
            db,
            oauth: OAuthConfig {
                github: None,
                google: None,
                discord: None,
            },
            jwt_secret: "test-secret".to_string(),
        };

        async fn handler(auth_user: AuthUser) -> Json<MeResponse> {
            Json(MeResponse {
                user_id: auth_user.user_id,
            })
        }

        let app = Router::new()
            .route("/test", get(handler))
            .with_state(state);

        let server = axum_test::TestServer::new(app).unwrap();
        let res = server.get("/test").await;

        res.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_user_extraction_with_invalid_token() {
        use axum::routing::get;
        use axum::Router;

        let db = crate::db::init_test_db().await.unwrap();
        let state = crate::AppState {
            db,
            oauth: OAuthConfig {
                github: None,
                google: None,
                discord: None,
            },
            jwt_secret: "test-secret".to_string(),
        };

        async fn handler(auth_user: AuthUser) -> Json<MeResponse> {
            Json(MeResponse {
                user_id: auth_user.user_id,
            })
        }

        let app = Router::new()
            .route("/test", get(handler))
            .with_state(state);

        let server = axum_test::TestServer::new(app).unwrap();
        let res = server
            .get("/test")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_static("Bearer invalid-jwt-token"),
            )
            .await;

        res.assert_status(StatusCode::UNAUTHORIZED);
    }
}
