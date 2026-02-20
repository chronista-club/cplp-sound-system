use std::f32::consts::PI;

use clap::Parser;
use cplp_audio::engine::AudioEngine;
use cplp_audio::midi_input::{self, MidiInputManager};
use cplp_audio::plugin_host;
use cplp_core::config::AudioConfig;

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
    /// インストール済み CLAP プラグインをスキャン
    Scan,
    /// MIDI 入力ポートを一覧
    Midi,
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
        Cli::Scan => {
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
        Cli::Midi => {
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
                println!("使い方: play <PLUGIN_ID> --midi <ポート番号>");
            }
        }
        Cli::Play {
            plugin_id,
            fx,
            midi,
            gui,
            duration,
        } => {
            let plugins = plugin_host::scan_plugins();

            // シンセプラグイン
            let plugin = plugins
                .iter()
                .find(|p| p.id == plugin_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "プラグイン '{}' が見つかりません。`scan` で ID を確認してください",
                        plugin_id
                    )
                })?;

            tracing::info!("Loading synth: {} ({})", plugin.name, plugin.id);

            let config = AudioConfig::default();
            let (mut synth_processor, mut note_ctrl, mut synth_handle) =
                plugin_host::load_plugin(
                    plugin,
                    config.sample_rate as f64,
                    config.buffer_size,
                    config.buffer_size,
                    config.channels as usize,
                )?;

            // エフェクトプラグイン（オプション）
            let fx_state = if let Some(ref fx_id) = fx {
                let fx_plugin = plugins
                    .iter()
                    .find(|p| p.id == *fx_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "エフェクトプラグイン '{}' が見つかりません",
                            fx_id
                        )
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
        Cli::Test { freq, duration } => {
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
    }

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
