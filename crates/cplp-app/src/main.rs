mod logging;

use std::f32::consts::PI;
use std::net::SocketAddr;

use clap::{Parser, Subcommand};
use cplp_audio::engine::AudioEngine;
use cplp_audio::midi_input::{self, MidiInputManager};
use cplp_audio::plugin_host;
use cplp_core::config::{AppConfig, AudioConfig, NetworkConfig};
use cplp_session::{LobbyClient, LobbyConfig, SessionManager};

#[derive(Parser)]
#[command(name = "cplp")]
#[command(about = "CLAP Plugin Live Performance - P2P リアルタイムジャムセッション")]
struct Cli {
    /// ログをファイルに出力
    #[arg(long, global = true)]
    log_file: Option<String>,

    #[command(subcommand)]
    cmd: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// CLAP プラグインをロードして演奏
    Play {
        /// シンセプラグイン ID（scan で確認）
        plugin_id: String,
        /// エフェクトプラグイン ID（シンセ出力にチェイン）
        #[arg(long)]
        fx: Option<String>,
        /// MIDI 入力ポート番号（midi コマンドで確認、省略時はテストノート）
        #[arg(short, long)]
        midi: Option<usize>,
        /// プラグインGUIを表示
        #[arg(short, long)]
        gui: bool,
        /// 再生時間 (秒、0 = 無制限)
        #[arg(short, long, default_value_t = 0)]
        duration: u64,
    },
    /// セッション管理（P2P 直接接続・ロビー経由）
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// デバイス情報（プラグイン・MIDI・オーディオ）
    Device {
        #[command(subcommand)]
        cmd: DeviceCmd,
    },
    /// HUD（ライブ演奏向け GUI）を起動
    Hud,
}

/// セッションサブコマンド
#[derive(Subcommand)]
enum SessionCmd {
    /// P2P ホストとして接続を待機
    Listen {
        /// リッスンポート
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
        /// シンセプラグイン ID
        plugin_id: String,
        /// エフェクトプラグイン ID
        #[arg(long)]
        fx: Option<String>,
        /// MIDI 入力ポート番号
        #[arg(short, long)]
        midi: Option<usize>,
    },
    /// P2P ピアに直接接続
    Connect {
        /// 接続先アドレス (例: [::1]:5000)
        addr: String,
        /// ローカルリッスンポート
        #[arg(short, long, default_value_t = 5001)]
        port: u16,
        /// シンセプラグイン ID
        plugin_id: String,
        /// エフェクトプラグイン ID
        #[arg(long)]
        fx: Option<String>,
        /// MIDI 入力ポート番号
        #[arg(short, long)]
        midi: Option<usize>,
    },
    /// ロビーサーバー経由でセッション管理
    Lobby {
        #[command(subcommand)]
        cmd: LobbyCmd,
    },
}

/// ロビーサブコマンド
#[derive(Subcommand)]
enum LobbyCmd {
    /// グループ一覧を取得
    Groups {
        /// ロビーサーバー URL
        #[arg(long, default_value = "http://localhost:3000")]
        url: String,
        /// JWT トークン（省略時は dev トークン生成）
        #[arg(long)]
        token: Option<String>,
    },
    /// ロビー経由でホストとしてセッション開始
    Host {
        /// グループ ID (例: testband)
        #[arg(long)]
        group: String,
        /// シンセプラグイン ID
        plugin_id: String,
        /// ローカル P2P ポート
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
        /// ロビーサーバー URL
        #[arg(long, default_value = "http://localhost:3000")]
        url: String,
        /// JWT トークン（省略時は dev トークン生成）
        #[arg(long)]
        token: Option<String>,
        /// MIDI 入力ポート番号
        #[arg(short, long)]
        midi: Option<usize>,
    },
    /// ロビー経由でセッションに参加
    Join {
        /// セッション ID (例: sessions:abc123)
        #[arg(long)]
        session: String,
        /// シンセプラグイン ID
        plugin_id: String,
        /// ローカル P2P ポート
        #[arg(short, long, default_value_t = 5001)]
        port: u16,
        /// ロビーサーバー URL
        #[arg(long, default_value = "http://localhost:3000")]
        url: String,
        /// JWT トークン（省略時は dev トークン生成）
        #[arg(long)]
        token: Option<String>,
        /// MIDI 入力ポート番号
        #[arg(short, long)]
        midi: Option<usize>,
    },
}

/// デバイスサブコマンド
#[derive(Subcommand)]
enum DeviceCmd {
    /// インストール済み CLAP プラグインをスキャン
    Scan,
    /// MIDI 入力ポートを一覧
    Midi,
    /// オーディオ出力テスト（サイン波）
    Test {
        /// 周波数 (Hz)
        #[arg(short, long, default_value_t = 440.0)]
        freq: f32,
        /// 再生時間 (秒)
        #[arg(short, long, default_value_t = 3)]
        duration: u64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let Some(cmd) = cli.cmd else {
        // ステータス表示時はログを抑制
        return show_status();
    };

    // サブコマンド実行時のみ tracing を初期化（non-blocking + プリセット対応）
    let _log_guards = logging::init_logging(cli.log_file.as_deref());

    match cmd {
        Command::Play {
            plugin_id,
            fx,
            midi,
            gui,
            duration,
        } => {
            let plugins = plugin_host::scan_plugins();

            // シンセプラグイン
            let plugin = plugins.iter().find(|p| p.id == plugin_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "プラグイン '{}' が見つかりません。`scan` で ID を確認してください",
                    plugin_id
                )
            })?;

            tracing::info!("Loading synth: {} ({})", plugin.name, plugin.id);

            let config = AudioConfig::default();
            let (mut synth_processor, mut note_ctrl, mut synth_handle) = plugin_host::load_plugin(
                plugin,
                config.sample_rate as f64,
                config.buffer_size,
                config.buffer_size,
                config.channels as usize,
            )?;

            // エフェクトプラグイン（オプション）
            let fx_state = if let Some(ref fx_id) = fx {
                let fx_plugin = plugins.iter().find(|p| p.id == *fx_id).ok_or_else(|| {
                    anyhow::anyhow!("エフェクトプラグイン '{}' が見つかりません", fx_id)
                })?;

                tracing::info!("Loading effect: {} ({})", fx_plugin.name, fx_plugin.id);

                let (fx_processor, _fx_note_ctrl, fx_handle) = plugin_host::load_plugin(
                    fx_plugin,
                    config.sample_rate as f64,
                    config.buffer_size,
                    config.buffer_size,
                    config.channels as usize,
                )?;

                println!("エフェクト: {}", fx_plugin.name);
                Some((fx_processor, fx_handle))
            } else {
                None
            };

            let mut engine = AudioEngine::new(config.clone());

            // エフェクトチェイン: synth → fx → output
            if let Some((mut fx_processor, _fx_handle)) = fx_state {
                let channels = config.channels as usize;
                let buf_size = config.buffer_size as usize * channels;
                engine.start(move |buf: &mut [f32]| {
                    // シンセ → 中間バッファ
                    let mut synth_out = vec![0.0f32; buf.len().max(buf_size)];
                    synth_processor.process(&mut synth_out[..buf.len()]);

                    // 中間バッファ → エフェクト → 出力
                    fx_processor.process_effect(&synth_out[..buf.len()], buf);
                })?;
            } else {
                engine.start(move |buf: &mut [f32]| {
                    synth_processor.process(buf);
                })?;
            }

            println!("プラグイン '{}' を再生中...", plugin.name);

            // MIDI or テストノートを先にセットアップ（run_gui はブロックするため）
            let _midi_conn = if let Some(port_index) = midi {
                let conn = MidiInputManager::connect(port_index, note_ctrl)?;
                println!("MIDI 入力接続済み — キーボードで演奏してください");
                Some(conn)
            } else {
                println!("C4 (MIDI note 60) テストノートを送信...");
                note_ctrl.note_on(60, 100);
                None
            };

            if gui {
                // GUI モード: winit イベントループでブロック（ウィンドウ閉じで終了）
                synth_handle.run_gui()?;
            } else {
                // 非GUI モード: 時間制限 or 無制限で待機
                wait_for_duration(duration);
            }

            engine.stop();
        }
        Command::Session { cmd: session_cmd } => match session_cmd {
            SessionCmd::Listen {
                port,
                plugin_id,
                fx,
                midi,
            } => {
                let (mut engine, _synth_handle, _fx_handle, _midi_conn) =
                    setup_session_audio(&plugin_id, fx.as_deref(), midi)?;

                println!("ピアの接続を待機中 (port {port})...");
                println!("(Ctrl+C で停止)");

                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(async {
                    let app_config = AppConfig {
                        audio: AudioConfig::default(),
                        network: NetworkConfig {
                            listen_port: port,
                            ..Default::default()
                        },
                    };
                    let mut session = SessionManager::new(app_config);

                    tokio::select! {
                        result = session.host() => {
                            match result {
                                Ok(_streamer) => {
                                    println!("セッション開始！");
                                    tokio::signal::ctrl_c().await.ok();
                                }
                                Err(e) => tracing::error!("セッションエラー: {}", e),
                            }
                        }
                        _ = tokio::signal::ctrl_c() => {
                            println!("\n停止中...");
                        }
                    }

                    session.shutdown().await.ok();
                });

                engine.stop();
            }
            SessionCmd::Connect {
                addr,
                port,
                plugin_id,
                fx,
                midi,
            } => {
                let peer_addr: SocketAddr = addr.parse()?;
                let (mut engine, _synth_handle, _fx_handle, _midi_conn) =
                    setup_session_audio(&plugin_id, fx.as_deref(), midi)?;

                println!("{} に接続中...", peer_addr);
                println!("(Ctrl+C で停止)");

                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(async {
                    let app_config = AppConfig {
                        audio: AudioConfig::default(),
                        network: NetworkConfig {
                            listen_port: port,
                            ..Default::default()
                        },
                    };
                    let mut session = SessionManager::new(app_config);

                    tokio::select! {
                        result = session.join(peer_addr) => {
                            match result {
                                Ok(_streamer) => {
                                    println!("セッション開始！");
                                    tokio::signal::ctrl_c().await.ok();
                                }
                                Err(e) => tracing::error!("セッションエラー: {}", e),
                            }
                        }
                        _ = tokio::signal::ctrl_c() => {
                            println!("\n停止中...");
                        }
                    }

                    session.shutdown().await.ok();
                });

                engine.stop();
            }
            SessionCmd::Lobby { cmd: lobby_cmd } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(handle_lobby(lobby_cmd))?;
            }
        },
        Command::Hud => {
            cplp_hud::app::run()?;
        }
        Command::Device { cmd: device_cmd } => match device_cmd {
            DeviceCmd::Scan => {
                let plugins = plugin_host::scan_plugins();
                if plugins.is_empty() {
                    println!("CLAP プラグインが見つかりません");
                    println!("インストール先: ~/Library/Audio/Plug-Ins/CLAP/");
                } else {
                    println!("{} 個のプラグインが見つかりました:\n", plugins.len());
                    for p in &plugins {
                        println!("  {} ({})", p.name, p.vendor);
                        println!("    ID: {}", p.id);
                        println!("    Version: {}", p.version);
                        println!("    Path: {}", p.bundle_path.display());
                        println!();
                    }
                }
            }
            DeviceCmd::Midi => {
                let ports = midi_input::list_midi_ports()?;
                if ports.is_empty() {
                    println!("MIDI 入力ポートが見つかりません");
                    println!("MIDI キーボードを接続してください");
                } else {
                    println!("{} 個の MIDI 入力ポート:\n", ports.len());
                    for (i, name) in ports.iter().enumerate() {
                        println!("  [{i}] {name}");
                    }
                    println!();
                    println!("使い方: cplp play <PLUGIN_ID> --midi <ポート番号>");
                }
            }
            DeviceCmd::Test { freq, duration } => {
                tracing::info!("Audio test: {freq}Hz for {duration}s");

                let config = AudioConfig::default();
                let sample_rate = config.sample_rate as f32;
                let channels = config.channels as usize;

                let mut phase: f32 = 0.0;
                let phase_inc = freq * 2.0 * PI / sample_rate;

                let mut engine = AudioEngine::new(config);
                engine.start(move |buf: &mut [f32]| {
                    for frame in buf.chunks_mut(channels) {
                        let sample = (phase).sin() * 0.3;
                        for ch in frame.iter_mut() {
                            *ch = sample;
                        }
                        phase += phase_inc;
                        if phase > 2.0 * PI {
                            phase -= 2.0 * PI;
                        }
                    }
                })?;

                std::thread::sleep(std::time::Duration::from_secs(duration));
                engine.stop();
            }
        },
    }

    Ok(())
}

// ─── ロビーコマンド ──────────────────────────────────────

async fn handle_lobby(cmd: LobbyCmd) -> anyhow::Result<()> {
    match cmd {
        LobbyCmd::Groups { url, token } => {
            let token = resolve_token(token)?;
            let lobby = LobbyClient::new(LobbyConfig {
                base_url: url,
                token,
                local_addr: "[::1]:0".parse().unwrap(),
            });

            let groups = lobby.list_groups().await?;
            if groups.is_empty() {
                println!("所属グループなし");
            } else {
                println!("{} 個のグループ:\n", groups.len());
                for g in &groups {
                    println!("  {} — {}", g.id, g.name);
                }
            }
        }
        LobbyCmd::Host {
            group,
            plugin_id,
            port,
            url,
            token,
            midi: _midi,
        } => {
            let token = resolve_token(token)?;
            let local_addr: SocketAddr = format!("[::1]:{port}").parse()?;

            let mut lobby = LobbyClient::new(LobbyConfig {
                base_url: url,
                token,
                local_addr,
            });

            // WebSocket 接続
            lobby.connect_ws().await?;

            let app_config = AppConfig {
                audio: AudioConfig::default(),
                network: NetworkConfig {
                    listen_port: port,
                    ..Default::default()
                },
            };

            // ロビーの user_id を取得するため JWT をデコード
            // (検証なし — dev 用途)
            let user_id = extract_user_id_from_token(lobby.config())?;
            let mut session = SessionManager::with_user_id(app_config, &user_id);

            println!("ロビー経由でホスト開始 (group: {group}, port: {port}, plugin: {plugin_id})");
            println!("ピアの参加を待機中... (Ctrl+C で停止)");

            tokio::select! {
                result = session.host_via_lobby(&mut lobby, &group) => {
                    match result {
                        Ok(_streamer) => {
                            println!("セッション開始！");
                            let _ = tokio::signal::ctrl_c().await;
                        }
                        Err(e) => tracing::error!("セッションエラー: {}", e),
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\n停止中...");
                }
            }

            session.shutdown().await.ok();
        }
        LobbyCmd::Join {
            session: session_id,
            plugin_id,
            port,
            url,
            token,
            midi: _midi,
        } => {
            let token = resolve_token(token)?;
            let local_addr: SocketAddr = format!("[::1]:{port}").parse()?;

            let mut lobby = LobbyClient::new(LobbyConfig {
                base_url: url,
                token,
                local_addr,
            });

            let app_config = AppConfig {
                audio: AudioConfig::default(),
                network: NetworkConfig {
                    listen_port: port,
                    ..Default::default()
                },
            };

            let user_id = extract_user_id_from_token(lobby.config())?;
            let mut session = SessionManager::with_user_id(app_config, &user_id);

            println!(
                "ロビー経由でセッション参加 (session: {session_id}, port: {port}, plugin: {plugin_id})"
            );

            tokio::select! {
                result = session.join_via_lobby(&mut lobby, &session_id) => {
                    match result {
                        Ok(_streamer) => {
                            println!("セッション開始！");
                            let _ = tokio::signal::ctrl_c().await;
                        }
                        Err(e) => tracing::error!("セッションエラー: {}", e),
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\n停止中...");
                }
            }

            session.shutdown().await.ok();
        }
    }
    Ok(())
}

/// トークンを解決: 明示指定があればそれを使い、なければ dev トークン生成
fn resolve_token(token: Option<String>) -> anyhow::Result<String> {
    match token {
        Some(t) => Ok(t),
        None => {
            let dev_secret = "dev-secret-do-not-use-in-production";
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let claims = serde_json::json!({
                "sub": "users:dev",
                "exp": now + 24 * 60 * 60,
                "iat": now,
            });
            let token = jsonwebtoken::encode(
                &jsonwebtoken::Header::default(),
                &claims,
                &jsonwebtoken::EncodingKey::from_secret(dev_secret.as_bytes()),
            )?;
            tracing::info!("dev トークン生成 (user: users:dev, secret: {dev_secret})");
            Ok(token)
        }
    }
}

/// JWT トークンから user_id (sub) を抽出
///
/// dev トークン（自己生成）の場合は署名検証をスキップし、
/// 明示指定トークンの場合は exp 検証のみ行う（署名検証はサーバー側で実施）。
/// NOTE: クライアント側では JWT secret を保持しないため完全な署名検証は不可。
fn extract_user_id_from_token(config: &LobbyConfig) -> anyhow::Result<String> {
    let mut validation = jsonwebtoken::Validation::default();
    // クライアント側は JWT secret を持たないため署名検証はサーバーに委任
    // ただし exp は検証して期限切れトークンを早期に弾く
    validation.insecure_disable_signature_validation();
    validation.validate_exp = true;
    let token_data = jsonwebtoken::decode::<serde_json::Value>(
        &config.token,
        &jsonwebtoken::DecodingKey::from_secret(&[]),
        &validation,
    )?;
    token_data.claims["sub"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("JWT に sub クレームがありません"))
}

// ─── ヘルパー関数 ──────────────────────────────────────

/// プラグインを ID で検索
fn find_plugin<'a>(
    plugins: &'a [plugin_host::PluginInfo],
    id: &str,
) -> anyhow::Result<&'a plugin_host::PluginInfo> {
    plugins.iter().find(|p| p.id == id).ok_or_else(|| {
        anyhow::anyhow!(
            "プラグイン '{}' が見つかりません。`scan` で ID を確認してください",
            id
        )
    })
}

/// セッション用のオーディオパイプラインをセットアップ
///
/// シンセ/FX プラグインのロード → AudioEngine 起動 → MIDI 接続。
/// 戻り値のハンドルはセッション終了まで保持すること（プラグインの生存期間管理）。
fn setup_session_audio(
    plugin_id: &str,
    fx_id: Option<&str>,
    midi_port: Option<usize>,
) -> anyhow::Result<(
    AudioEngine,
    plugin_host::PluginHandle,
    Option<plugin_host::PluginHandle>,
    Option<MidiInputManager>,
)> {
    let config = AudioConfig::default();
    let plugins = plugin_host::scan_plugins();

    // シンセプラグイン
    let plugin = find_plugin(&plugins, plugin_id)?;
    tracing::info!("Loading synth: {} ({})", plugin.name, plugin.id);
    let (mut synth_processor, note_ctrl, synth_handle) = plugin_host::load_plugin(
        plugin,
        config.sample_rate as f64,
        config.buffer_size,
        config.buffer_size,
        config.channels as usize,
    )?;
    println!("シンセ: {}", plugin.name);

    // FX プラグイン（オプション）
    let (fx_processor, fx_handle) = if let Some(fx_id) = fx_id {
        let fx_plugin = find_plugin(&plugins, fx_id)?;
        tracing::info!("Loading effect: {} ({})", fx_plugin.name, fx_plugin.id);
        let (proc_, _, handle) = plugin_host::load_plugin(
            fx_plugin,
            config.sample_rate as f64,
            config.buffer_size,
            config.buffer_size,
            config.channels as usize,
        )?;
        println!("エフェクト: {}", fx_plugin.name);
        (Some(proc_), Some(handle))
    } else {
        (None, None)
    };

    // AudioEngine 起動
    let mut engine = AudioEngine::new(config.clone());
    if let Some(mut fx_processor) = fx_processor {
        let channels = config.channels as usize;
        let buf_size = config.buffer_size as usize * channels;
        engine.start(move |buf: &mut [f32]| {
            let mut synth_out = vec![0.0f32; buf.len().max(buf_size)];
            synth_processor.process(&mut synth_out[..buf.len()]);
            fx_processor.process_effect(&synth_out[..buf.len()], buf);
        })?;
    } else {
        engine.start(move |buf: &mut [f32]| {
            synth_processor.process(buf);
        })?;
    }

    // MIDI 接続
    let midi_conn = if let Some(port_index) = midi_port {
        let conn = MidiInputManager::connect(port_index, note_ctrl)?;
        println!("MIDI 入力接続済み");
        Some(conn)
    } else {
        None
    };

    Ok((engine, synth_handle, fx_handle, midi_conn))
}

// ─── ステータス表示 ──────────────────────────────────────

/// 引数なしで起動した際にシステムステータスを表示
fn show_status() -> anyhow::Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!("  cplp v{version}");
    println!("  CLAP Plugin Live Performance - P2P リアルタイムジャムセッション");
    println!();

    // Plugins
    let plugins = plugin_host::scan_plugins();
    if plugins.is_empty() {
        println!("  Plugins:  (none)");
    } else {
        println!("  Plugins:  {} detected", plugins.len());
        for p in &plugins {
            println!("    - {} ({})", p.name, p.id);
        }
    }
    println!();

    // MIDI
    match midi_input::list_midi_ports() {
        Ok(ports) if !ports.is_empty() => {
            println!("  MIDI:     {} ports", ports.len());
            for (i, name) in ports.iter().enumerate() {
                println!("    - [{i}] {name}");
            }
        }
        _ => {
            println!("  MIDI:     (none)");
        }
    }
    println!();

    // Audio
    let config = AudioConfig::default();
    println!(
        "  Audio:    {}Hz, {}ch, buffer {}",
        config.sample_rate, config.channels, config.buffer_size
    );
    println!();

    // Quick start
    println!("  Quick start:");
    if let Some(p) = plugins.first() {
        println!("    cplp play {}          演奏", p.id);
        println!("    cplp play {} --midi 0  MIDI で演奏", p.id);
    } else {
        println!("    cplp play <PLUGIN_ID>              演奏");
    }
    println!("    cplp session lobby host --group <ID> ...  ロビー経由セッション");
    println!();
    println!("  Run `cplp --help` for all commands.");
    println!();

    Ok(())
}

fn wait_for_duration(duration: u64) {
    if duration > 0 {
        println!("({duration}秒後に停止)");
        std::thread::sleep(std::time::Duration::from_secs(duration));
    } else {
        wait_forever();
    }
}

fn wait_forever() {
    println!("(Ctrl+C で停止)");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用 JWT を生成（任意の claims で）
    fn make_token(claims: serde_json::Value) -> String {
        jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"test-secret"),
        )
        .unwrap()
    }

    fn make_config(token: &str) -> LobbyConfig {
        LobbyConfig {
            base_url: "http://localhost:3000".into(),
            token: token.to_string(),
            local_addr: "[::1]:5000".parse().unwrap(),
        }
    }

    // -- extract_user_id_from_token --

    #[test]
    fn test_extract_user_id_valid_token() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = make_token(serde_json::json!({
            "sub": "users:player1",
            "exp": now + 3600,
            "iat": now,
        }));
        let config = make_config(&token);
        let result = extract_user_id_from_token(&config).unwrap();
        assert_eq!(result, "users:player1");
    }

    #[test]
    fn test_extract_user_id_expired_token() {
        // exp を過去の固定値に設定して期限切れを検証
        let token = make_token(serde_json::json!({
            "sub": "users:expired",
            "exp": 1_000_000_000_u64,
            "iat": 999_999_000_u64,
        }));
        let config = make_config(&token);
        let result = extract_user_id_from_token(&config);
        assert!(result.is_err(), "期限切れトークンはエラーになるべき");
    }

    #[test]
    fn test_extract_user_id_no_sub_claim() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = make_token(serde_json::json!({
            "exp": now + 3600,
            "iat": now,
        }));
        let config = make_config(&token);
        let result = extract_user_id_from_token(&config);
        assert!(
            result.is_err(),
            "sub クレームがないトークンはエラーになるべき"
        );
        assert!(result.unwrap_err().to_string().contains("sub"));
    }

    #[test]
    fn test_extract_user_id_garbage_token() {
        let config = make_config("not-a-jwt-token");
        let result = extract_user_id_from_token(&config);
        assert!(result.is_err(), "不正なトークンはエラーになるべき");
    }

    // -- resolve_token --

    #[test]
    fn test_resolve_token_explicit() {
        let token = resolve_token(Some("my-explicit-token".into())).unwrap();
        assert_eq!(token, "my-explicit-token");
    }

    #[test]
    fn test_resolve_token_dev_generation() {
        let token = resolve_token(None).unwrap();
        // dev トークンは有効な JWT 形式（header.payload.signature）
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT は 3 パートであるべき");

        // 生成されたトークンで user_id を抽出できることも検証
        let config = make_config(&token);
        let user_id = extract_user_id_from_token(&config).unwrap();
        assert_eq!(user_id, "users:dev");
    }
}
