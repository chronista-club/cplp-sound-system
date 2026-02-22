mod logging;

use std::f32::consts::PI;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use clap::{Parser, Subcommand};
use cplp_audio::engine::AudioEngine;
use cplp_audio::midi_input::{self, MidiInputManager};
use cplp_audio::plugin_host;
use cplp_core::config::{AppConfig, AudioConfig, NetworkConfig};
use cplp_hud::{HudAction, HudBridge, HudContext, HudLiveData, PluginEntry, app_status};
use cplp_hud::state::{AudioMeters, PcmSnapshot, PcmWriter, SessionSnapshot};
use cplp_network::control::{CommandMode, ControlEvent};
use cplp_session::{LobbyClient, LobbyConfig, SessionManager, SessionState};

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
        /// HUD（ライブ演奏 GUI）を同時起動
        #[arg(long)]
        hud: bool,
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
        /// HUD（ライブ演奏 GUI）を同時起動
        #[arg(long)]
        hud: bool,
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
                hud,
            } => {
                let meters = if hud {
                    Some(Arc::new(AudioMeters::default()))
                } else {
                    None
                };
                let (pcm_writer, pcm_output) = if hud {
                    let (input, output) = triple_buffer::triple_buffer(&PcmSnapshot::default());
                    (Some(PcmWriter::new(input)), Some(output))
                } else {
                    (None, None)
                };
                let (mut engine, _synth_handle, _fx_handle, _midi_conn) = setup_session_audio(
                    &plugin_id,
                    fx.as_deref(),
                    midi,
                    meters.clone(),
                    pcm_writer,
                )?;

                println!("ピアの接続を待機中 (port {port})...");
                println!("(Ctrl+C で停止)");

                let app_config = AppConfig {
                    audio: AudioConfig::default(),
                    network: NetworkConfig {
                        listen_port: port,
                        ..Default::default()
                    },
                };

                if let Some(meters) = meters {
                    run_session_with_hud(
                        &mut engine,
                        meters,
                        pcm_output.unwrap(),
                        app_config,
                        SessionMode::Host,
                    )?;
                } else {
                    run_session_blocking(&mut engine, app_config, SessionMode::Host)?;
                }
            }
            SessionCmd::Connect {
                addr,
                port,
                plugin_id,
                fx,
                midi,
                hud,
            } => {
                let peer_addr: SocketAddr = addr.parse()?;
                let meters = if hud {
                    Some(Arc::new(AudioMeters::default()))
                } else {
                    None
                };
                let (pcm_writer, pcm_output) = if hud {
                    let (input, output) = triple_buffer::triple_buffer(&PcmSnapshot::default());
                    (Some(PcmWriter::new(input)), Some(output))
                } else {
                    (None, None)
                };
                let (mut engine, _synth_handle, _fx_handle, _midi_conn) = setup_session_audio(
                    &plugin_id,
                    fx.as_deref(),
                    midi,
                    meters.clone(),
                    pcm_writer,
                )?;

                println!("{} に接続中...", peer_addr);
                println!("(Ctrl+C で停止)");

                let app_config = AppConfig {
                    audio: AudioConfig::default(),
                    network: NetworkConfig {
                        listen_port: port,
                        ..Default::default()
                    },
                };

                if let Some(meters) = meters {
                    run_session_with_hud(
                        &mut engine,
                        meters,
                        pcm_output.unwrap(),
                        app_config,
                        SessionMode::Join(peer_addr),
                    )?;
                } else {
                    run_session_blocking(&mut engine, app_config, SessionMode::Join(peer_addr))?;
                }
            }
            SessionCmd::Lobby { cmd: lobby_cmd } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(handle_lobby(lobby_cmd))?;
            }
        },
        Command::Hud => {
            run_interactive_hud()?;
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
    meters: Option<Arc<AudioMeters>>,
    pcm_writer: Option<PcmWriter>,
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

    // AudioEngine 起動（meters + PCM writer があれば callback 内で書き込む）
    let mut engine = AudioEngine::new(config.clone());
    if let Some(mut fx_processor) = fx_processor {
        let channels = config.channels as usize;
        let buf_size = config.buffer_size as usize * channels;
        let m = meters.clone();
        let mut pcm = pcm_writer;
        engine.start(move |buf: &mut [f32]| {
            let mut synth_out = vec![0.0f32; buf.len().max(buf_size)];
            synth_processor.process(&mut synth_out[..buf.len()]);
            fx_processor.process_effect(&synth_out[..buf.len()], buf);
            if let Some(m) = &m {
                write_meters(m, buf);
            }
            if let Some(pcm) = &mut pcm {
                pcm.push(buf);
            }
        })?;
    } else {
        let m = meters;
        let mut pcm = pcm_writer;
        engine.start(move |buf: &mut [f32]| {
            synth_processor.process(buf);
            if let Some(m) = &m {
                write_meters(m, buf);
            }
            if let Some(pcm) = &mut pcm {
                pcm.push(buf);
            }
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

/// オーディオバッファから RMS レベルを計算し AudioMeters に書き込む
fn write_meters(meters: &AudioMeters, buf: &[f32]) {
    if buf.is_empty() {
        return;
    }
    let sum: f32 = buf.iter().map(|s| s * s).sum();
    let rms = (sum / buf.len() as f32).sqrt();
    meters.local_level.store(rms, Relaxed);

    let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    meters.local_peak.store(peak, Relaxed);
}

/// SessionState を HUD 用の SessionSnapshot に変換
fn session_state_to_snapshot(state: &SessionState) -> SessionSnapshot {
    match state {
        SessionState::Streaming => SessionSnapshot {
            connected: true,
            peer_name: "Peer".into(),
            ..Default::default()
        },
        SessionState::Connected => SessionSnapshot {
            connected: true,
            peer_name: "Peer (connecting...)".into(),
            ..Default::default()
        },
        _ => SessionSnapshot::default(),
    }
}

/// セッション接続モード
enum SessionMode {
    /// ホスト（ピアの接続を待機）
    Host,
    /// ゲスト（指定アドレスに接続）
    Join(SocketAddr),
}

/// HUD 付きでセッションを実行（メインスレッドで winit、バックグラウンドで tokio）
fn run_session_with_hud(
    engine: &mut AudioEngine,
    meters: Arc<AudioMeters>,
    local_pcm: triple_buffer::Output<PcmSnapshot>,
    app_config: AppConfig,
    mode: SessionMode,
) -> anyhow::Result<()> {
    let (mut buf_input, buf_output) = triple_buffer::triple_buffer(&SessionSnapshot::default());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let mut session = SessionManager::new(app_config);
            let state_rx = session.state_rx();

            let _streamer = match mode {
                SessionMode::Host => session.host().await,
                SessionMode::Join(addr) => session.join(addr).await,
            };

            // セッション状態の転送とコマンド入力を並行実行
            let mut watch = state_rx;
            tokio::select! {
                _ = async {
                    // セッション状態を TripleBuffer に転送し続ける
                    loop {
                        if watch.changed().await.is_err() {
                            break;
                        }
                        let state = watch.borrow().clone();
                        buf_input.write(session_state_to_snapshot(&state));
                        if matches!(state, SessionState::Disconnected) {
                            break;
                        }
                    }
                } => {}
                _ = command_input_loop(&session) => {
                    tracing::info!("コマンド入力ループが終了");
                }
            }
            session.shutdown().await.ok();
        });
    });

    // HUD をメインスレッドで起動（winit 要件）
    let ctx = HudContext {
        meters,
        session: buf_output,
        local_pcm,
    };
    cplp_hud::app::run_with_context(ctx)?;
    engine.stop();
    Ok(())
}

/// インタラクティブ HUD を起動
///
/// 1. プラグイン/MIDI ポートをスキャン
/// 2. HudBridge を構築
/// 3. バックグラウンドスレッドで HudAction を処理
/// 4. メインスレッドで run_interactive() を実行（winit 要件）
fn run_interactive_hud() -> anyhow::Result<()> {
    // ── 1. スキャン ──
    let plugins = plugin_host::scan_plugins();
    let midi_ports = midi_input::list_midi_ports().unwrap_or_default();

    let plugin_entries: Vec<PluginEntry> = plugins
        .iter()
        .map(|p| PluginEntry {
            name: p.name.clone(),
            id: p.id.clone(),
            vendor: p.vendor.clone(),
        })
        .collect();

    // ── 2. HudBridge 構築 ──
    let (action_tx, action_rx) = std::sync::mpsc::channel::<HudAction>();
    let status = Arc::new(std::sync::atomic::AtomicU8::new(app_status::READY));
    let status_message = Arc::new(std::sync::Mutex::new(String::new()));
    let live_data: Arc<std::sync::Mutex<Option<HudLiveData>>> =
        Arc::new(std::sync::Mutex::new(None));

    let bridge = HudBridge {
        plugins: plugin_entries,
        midi_ports: midi_ports.clone(),
        action_tx,
        status: Arc::clone(&status),
        status_message: Arc::clone(&status_message),
        live_data: Arc::clone(&live_data),
    };

    // ── 3. バックグラウンドスレッド ──
    let bg_status = Arc::clone(&status);
    let bg_status_message = Arc::clone(&status_message);
    let bg_live_data = Arc::clone(&live_data);

    std::thread::spawn(move || {
        // 現在のエンジン・MIDI 接続を保持（Stop 時に解放）
        let mut current_engine: Option<AudioEngine> = None;
        let mut _current_midi: Option<MidiInputManager> = None;
        let mut _current_synth_handle: Option<plugin_host::PluginHandle> = None;

        while let Ok(action) = action_rx.recv() {
            match action {
                HudAction::Play {
                    plugin_index,
                    midi_port_index,
                } => {
                    // 既存のエンジンを停止
                    if let Some(mut engine) = current_engine.take() {
                        engine.stop();
                    }
                    _current_midi = None;
                    _current_synth_handle = None;
                    if let Ok(mut ld) = bg_live_data.lock() {
                        *ld = None;
                    }

                    bg_status.store(app_status::LOADING, Relaxed);
                    if let Ok(mut msg) = bg_status_message.lock() {
                        *msg = "Loading plugin".into();
                    }

                    // プラグイン検索
                    let Some(plugin_info) = plugins.get(plugin_index) else {
                        bg_status.store(app_status::ERROR, Relaxed);
                        if let Ok(mut msg) = bg_status_message.lock() {
                            *msg = format!("Plugin index {} out of range", plugin_index);
                        }
                        continue;
                    };

                    tracing::info!(
                        "Interactive: loading {} ({})",
                        plugin_info.name,
                        plugin_info.id
                    );

                    // meters + PCM writer
                    let meters = Arc::new(AudioMeters::default());
                    let (pcm_input, pcm_output) =
                        triple_buffer::triple_buffer(&PcmSnapshot::default());
                    let mut pcm_writer = PcmWriter::new(pcm_input);

                    // プラグインロード
                    let config = cplp_core::config::AudioConfig::default();
                    let load_result = plugin_host::load_plugin(
                        plugin_info,
                        config.sample_rate as f64,
                        config.buffer_size,
                        config.buffer_size,
                        config.channels as usize,
                    );

                    let (mut synth_processor, note_ctrl, synth_handle) = match load_result {
                        Ok(r) => r,
                        Err(e) => {
                            bg_status.store(app_status::ERROR, Relaxed);
                            if let Ok(mut msg) = bg_status_message.lock() {
                                *msg = format!("Plugin load failed: {e}");
                            }
                            continue;
                        }
                    };

                    // AudioEngine 起動
                    let mut engine = AudioEngine::new(config);
                    let m = Arc::clone(&meters);
                    let start_result = engine.start(move |buf: &mut [f32]| {
                        synth_processor.process(buf);
                        write_meters(&m, buf);
                        pcm_writer.push(buf);
                    });

                    if let Err(e) = start_result {
                        bg_status.store(app_status::ERROR, Relaxed);
                        if let Ok(mut msg) = bg_status_message.lock() {
                            *msg = format!("Audio engine failed: {e}");
                        }
                        continue;
                    }

                    // MIDI 接続
                    let midi_conn = if let Some(port_idx) = midi_port_index {
                        match MidiInputManager::connect(port_idx, note_ctrl) {
                            Ok(conn) => Some(conn),
                            Err(e) => {
                                tracing::warn!("MIDI connect failed: {e}");
                                None
                            }
                        }
                    } else {
                        None
                    };

                    // live_data をセット
                    if let Ok(mut ld) = bg_live_data.lock() {
                        *ld = Some(HudLiveData {
                            meters,
                            local_pcm: pcm_output,
                        });
                    }

                    current_engine = Some(engine);
                    _current_midi = midi_conn;
                    _current_synth_handle = Some(synth_handle);

                    bg_status.store(app_status::PLAYING, Relaxed);
                    if let Ok(mut msg) = bg_status_message.lock() {
                        *msg = format!("Playing: {}", plugin_info.name);
                    }
                }
                HudAction::Stop => {
                    if let Some(mut engine) = current_engine.take() {
                        engine.stop();
                    }
                    _current_midi = None;
                    _current_synth_handle = None;
                    if let Ok(mut ld) = bg_live_data.lock() {
                        *ld = None;
                    }
                    bg_status.store(app_status::READY, Relaxed);
                    if let Ok(mut msg) = bg_status_message.lock() {
                        msg.clear();
                    }
                }
            }
        }

        // チャネル切断 = HUD ウィンドウ閉じ → クリーンアップ
        if let Some(mut engine) = current_engine.take() {
            engine.stop();
        }
    });

    // ── 4. メインスレッドで HUD 起動 ──
    cplp_hud::run_interactive(bridge)
}

/// stdin の1行をパースして (CommandMode, text) を返す
///
/// - `/ask <text>` → `CommandMode::Ask`
/// - `/parse <text>` → `CommandMode::Parse`
/// - その他の非空行 → `CommandMode::Parse`（デフォルト）
/// - 空行 → `None`（スキップ）
fn parse_command_line(line: &str) -> Option<(CommandMode, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // "/ask" のみ（テキストなし）もスキップ
    if trimmed == "/ask" || trimmed == "/parse" {
        return None;
    }

    if let Some(text) = trimmed.strip_prefix("/ask ") {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some((CommandMode::Ask, text.to_string()))
    } else if let Some(text) = trimmed.strip_prefix("/parse ") {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some((CommandMode::Parse, text.to_string()))
    } else {
        Some((CommandMode::Parse, trimmed.to_string()))
    }
}

/// stdin からコマンドを読み取り、ControlEvent::Command を全ピアに送信するループ
///
/// セッション中に `/parse <text>` や `/ask <text>` を入力すると、
/// Cadence（ピア）に ControlEvent::Command を broadcast する。
async fn command_input_loop(session: &SessionManager) {
    let stdin = tokio::io::stdin();
    let reader = tokio::io::BufReader::new(stdin);

    use tokio::io::AsyncBufReadExt;
    let mut lines = reader.lines();

    println!("コマンド入力待機中（/parse <text>, /ask <text>, または直接テキスト入力）");

    while let Ok(Some(line)) = lines.next_line().await {
        let Some((mode, text)) = parse_command_line(&line) else {
            continue;
        };

        let local_id = session.p2p().local_peer_id().clone();
        let event = ControlEvent::Command {
            from: local_id,
            mode,
            text: text.clone(),
        };

        // control チャネルのマップを構築して broadcast
        // UnisonChannel は Clone 未実装のため、peers() から直接参照で broadcast する
        // TODO: UnisonChannel が Clone 対応したら HashMap<PeerId, UnisonChannel> を構築して
        //       ControlHandler::broadcast() を使う。現時点では peers() の control チャネルを
        //       直接イテレートして send_event する。
        let json = match serde_json::to_value(&event) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("コマンドのシリアライズに失敗: {}", e);
                continue;
            }
        };

        let peers = session.p2p().peers();
        let mut sent = 0usize;
        for (peer_id, conn) in peers.iter() {
            if let Some(ref channels) = conn.channels {
                if let Err(e) = channels.control.send_event("control", json.clone()).await {
                    tracing::warn!("コマンド送信失敗 (peer: {}): {}", peer_id, e);
                } else {
                    sent += 1;
                }
            }
        }

        if sent > 0 {
            tracing::info!("コマンド送信完了: {} ピアに配信 (text: {})", sent, text);
        } else {
            println!("接続中のピアがありません。コマンドは送信されませんでした。");
        }
    }
}

/// HUD なしでセッションを実行（tokio ランタイムでブロック）
fn run_session_blocking(
    engine: &mut AudioEngine,
    app_config: AppConfig,
    mode: SessionMode,
) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut session = SessionManager::new(app_config);

        let result = match mode {
            SessionMode::Host => session.host().await,
            SessionMode::Join(addr) => session.join(addr).await,
        };

        match result {
            Ok(_streamer) => {
                println!("セッション開始！");
                // streamer を保持しつつ、stdin コマンドループと Ctrl+C を並行実行
                tokio::select! {
                    _ = command_input_loop(&session) => {
                        tracing::info!("コマンド入力ループが終了");
                    }
                    _ = tokio::signal::ctrl_c() => {
                        println!("\n停止中...");
                    }
                }
            }
            Err(e) => tracing::error!("セッションエラー: {}", e),
        }

        session.shutdown().await.ok();
    });
    engine.stop();
    Ok(())
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

    // -- parse_command_line --

    #[test]
    fn test_parse_command_line_ask() {
        let result = parse_command_line("/ask C major chord");
        let (mode, text) = result.expect("Some を返すべき");
        assert!(matches!(mode, CommandMode::Ask));
        assert_eq!(text, "C major chord");
    }

    #[test]
    fn test_parse_command_line_parse() {
        let result = parse_command_line("/parse C4 E4 G4 120bpm");
        let (mode, text) = result.expect("Some を返すべき");
        assert!(matches!(mode, CommandMode::Parse));
        assert_eq!(text, "C4 E4 G4 120bpm");
    }

    #[test]
    fn test_parse_command_line_default_mode() {
        let result = parse_command_line("C major scale");
        let (mode, text) = result.expect("Some を返すべき");
        assert!(matches!(mode, CommandMode::Parse), "デフォルトは Parse");
        assert_eq!(text, "C major scale");
    }

    #[test]
    fn test_parse_command_line_empty() {
        assert!(parse_command_line("").is_none());
        assert!(parse_command_line("   ").is_none());
        assert!(parse_command_line("\n").is_none());
    }

    #[test]
    fn test_parse_command_line_ask_empty_text() {
        assert!(parse_command_line("/ask ").is_none());
        assert!(parse_command_line("/ask   ").is_none());
    }

    #[test]
    fn test_parse_command_line_parse_empty_text() {
        assert!(parse_command_line("/parse ").is_none());
        assert!(parse_command_line("/parse   ").is_none());
    }

    #[test]
    fn test_parse_command_line_with_leading_whitespace() {
        let result = parse_command_line("  /ask what is a tritone?  ");
        let (mode, text) = result.expect("Some を返すべき");
        assert!(matches!(mode, CommandMode::Ask));
        assert_eq!(text, "what is a tritone?");
    }
}
