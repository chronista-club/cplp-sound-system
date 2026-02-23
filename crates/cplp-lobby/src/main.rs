use cplp_lobby::{AppState, LobbyMode, auth, create_router, db, jwt};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // トレーシング初期化
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // ロビーモード判定
    let lobby_mode = match std::env::var("LOBBY_MODE")
        .unwrap_or_else(|_| "local".to_string())
        .as_str()
    {
        "global" => LobbyMode::Global,
        _ => LobbyMode::Local,
    };

    tracing::info!("Lobby mode: {:?}", lobby_mode);

    // SurrealDB 接続
    let db = match lobby_mode {
        LobbyMode::Local => {
            // LOCAL: インメモリ DB（ゼロコンフィグ）
            tracing::info!("Using in-memory database (local mode)");
            db::init_db().await?
        }
        LobbyMode::Global => {
            // GLOBAL: 環境変数の SURREAL_URL を使用
            db::init_db().await?
        }
    };

    // OAuth 設定
    let base_url =
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let oauth = auth::init_oauth_config(&base_url);

    // JWT シークレット
    let jwt_secret = match lobby_mode {
        LobbyMode::Local => {
            let secret = "dev-secret-do-not-use-in-production".to_string();
            tracing::info!("Using dev JWT secret (local mode)");

            // LOCAL モード: dev トークンを自動発行してログに表示
            let dev_token = jwt::create_token("users:dev", &secret)?;
            tracing::info!("Dev token (user: users:dev): {}", dev_token);

            secret
        }
        LobbyMode::Global => std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            tracing::warn!("JWT_SECRET not set, using development fallback");
            "dev-secret-do-not-use-in-production".to_string()
        }),
    };

    let state = AppState {
        db,
        oauth,
        jwt_secret,
        lobby_mode,
    };

    let app = create_router(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let bind_addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!("cplp-lobby listening on {} (mode: {:?})", bind_addr, lobby_mode);

    axum::serve(listener, app).await?;

    Ok(())
}
