#[allow(dead_code)]
mod midi_types;
#[allow(dead_code)]
mod parser;
#[allow(dead_code)]
mod router;
#[allow(dead_code)]
mod sequencer;
mod session;

use clap::{Parser, Subcommand};

use session::CadenceSession;

#[derive(Parser)]
#[command(name = "cadence")]
#[command(about = "Cadence - AI バンドメンバー")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// セッションをホストして接続を待機
    Listen {
        /// CLAP プラグイン ID
        plugin_id: String,
        /// 待機ポート
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
    /// 指定アドレスに接続
    Connect {
        /// 接続先アドレス
        addr: String,
        /// CLAP プラグイン ID
        plugin_id: String,
        /// ローカルポート
        #[arg(short, long, default_value_t = 5001)]
        port: u16,
    },
    /// 稼働状況を表示
    Status,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    match cli.cmd {
        Command::Listen { plugin_id, port } => {
            let session = CadenceSession::new(plugin_id, port);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(session.run_listen())?;
        }
        Command::Connect {
            addr,
            plugin_id,
            port,
        } => {
            let addr: std::net::SocketAddr = addr
                .parse()
                .map_err(|e| anyhow::anyhow!("アドレスのパースに失敗: {e}"))?;
            let session = CadenceSession::new(plugin_id, port);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(session.run_connect(addr))?;
        }
        Command::Status => {
            println!("Cadence is not running");
        }
    }
    Ok(())
}
