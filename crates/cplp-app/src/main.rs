use clap::Parser;

#[derive(Parser)]
#[command(name = "cplp-sound-system")]
#[command(about = "CLAP Plugin Live Performance - P2P リアルタイムジャムセッション")]
enum Cli {
    /// サーバーを起動して接続を待機
    Listen {
        /// リッスンポート
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
    /// 相手のピアに接続
    Connect {
        /// 接続先アドレス (例: [::1]:5000)
        addr: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli {
        Cli::Listen { port } => {
            tracing::info!("Listening on [::]:{port}");
            // TODO: SessionManager 起動
        }
        Cli::Connect { addr } => {
            tracing::info!("Connecting to {addr}");
            // TODO: SessionManager 起動 + 接続
        }
    }

    Ok(())
}
