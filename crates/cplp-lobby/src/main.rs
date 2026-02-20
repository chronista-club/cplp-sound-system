use cplp_lobby::{AppState, auth, create_router, db};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // トレーシング初期化
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // SurrealDB 接続
    let db = db::init_db().await?;

    // OAuth 設定
    let base_url =
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let oauth = auth::init_oauth_config(&base_url);

    // JWT シークレット
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| {
            tracing::warn!("JWT_SECRET not set, using development fallback");
            "dev-secret-do-not-use-in-production".to_string()
        });

    let state = AppState {
        db,
        oauth,
        jwt_secret,
    };

    let app = create_router(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("cplp-lobby listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}
