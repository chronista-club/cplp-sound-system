use cplp_lobby::create_router;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // トレーシング初期化
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let app = create_router();

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("cplp-lobby listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}
