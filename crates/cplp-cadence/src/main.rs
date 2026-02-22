use clap::{Parser, Subcommand};

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
    let cli = Cli::parse();
    match cli.cmd {
        Command::Listen { plugin_id, port } => {
            println!("Cadence listen on :{port} with plugin {plugin_id}");
        }
        Command::Connect {
            addr,
            plugin_id,
            port,
        } => {
            println!("Cadence connect to {addr} from :{port} with plugin {plugin_id}");
        }
        Command::Status => {
            println!("Cadence is not running");
        }
    }
    Ok(())
}
