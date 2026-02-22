use std::net::SocketAddr;
use std::time::Instant;

use anyhow::{Context, Result};
use tracing::{error, info};

use cplp_audio::engine::AudioEngine;
use cplp_audio::plugin_host::{self, NoteController, PluginInfo};
use cplp_core::config::{AppConfig, AudioConfig, NetworkConfig};
use cplp_session::manager::SessionManager;

use crate::sequencer::{MidiSequencer, NoteCommand};

/// Cadence セッション: プラグインロード → P2P接続 → シーケンサーループ
pub struct CadenceSession {
    plugin_id: String,
    port: u16,
}

impl CadenceSession {
    pub fn new(plugin_id: String, port: u16) -> Self {
        Self { plugin_id, port }
    }

    /// インストール済みプラグインから ID/名前で検索
    fn find_plugin(&self) -> Result<PluginInfo> {
        let plugins = plugin_host::scan_plugins();
        if plugins.is_empty() {
            anyhow::bail!("CLAP プラグインが見つかりません");
        }

        // ID 完全一致 → 名前部分一致 の順で検索
        let needle = &self.plugin_id;
        let found = plugins
            .iter()
            .find(|p| p.id == *needle)
            .or_else(|| {
                plugins.iter().find(|p| {
                    p.name.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
                })
            })
            .cloned();

        match found {
            Some(plugin) => {
                info!(
                    "プラグイン発見: {} ({}) [{}]",
                    plugin.name, plugin.id, plugin.vendor
                );
                Ok(plugin)
            }
            None => {
                // 利用可能なプラグインを列挙して表示
                eprintln!("利用可能なプラグイン:");
                for p in &plugins {
                    eprintln!("  {} - {} ({})", p.id, p.name, p.vendor);
                }
                anyhow::bail!("プラグイン '{}' が見つかりません", needle);
            }
        }
    }

    /// AppConfig を構築
    fn build_config(&self) -> AppConfig {
        AppConfig {
            audio: AudioConfig::default(),
            network: NetworkConfig {
                listen_port: self.port,
                ..NetworkConfig::default()
            },
        }
    }

    /// ホストとしてセッションを開始し、接続を待機
    pub async fn run_listen(&self) -> Result<()> {
        // 1. プラグインの検索とロード
        let plugin_info = self.find_plugin()?;
        let config = self.build_config();

        let sample_rate = config.audio.sample_rate as f64;
        let buffer_size = config.audio.buffer_size;
        let channels = config.audio.channels as usize;

        let (mut processor, note_ctrl, _handle) = plugin_host::load_plugin(
            &plugin_info,
            sample_rate,
            buffer_size,
            buffer_size,
            channels,
        )
        .context("プラグインのロードに失敗")?;

        info!("プラグインをロード: {}", plugin_info.name);

        // 2. AudioEngine を起動
        let mut engine = AudioEngine::new(config.audio.clone());
        engine
            .start(move |buf| {
                processor.process(buf);
            })
            .context("オーディオエンジンの起動に失敗")?;

        info!("オーディオエンジン起動完了");

        // 3. SessionManager でホスト開始
        let mut session = SessionManager::new(config);
        println!(
            "Cadence: ポート {} で待機中... (Ctrl+C で停止)",
            self.port
        );

        // host() は PeerConnected を待つ。接続されるまでブロックする。
        // 接続前に Ctrl+C が押された場合は tokio::signal で処理する。
        tokio::select! {
            result = session.host() => {
                match result {
                    Ok(_streamer) => {
                        info!("ピアが接続しました。シーケンサーループを開始...");
                        self.run_sequencer_loop(note_ctrl).await?;
                    }
                    Err(e) => {
                        error!("セッション開始に失敗: {e}");
                        return Err(e.into());
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C: セッションを終了します");
            }
        }

        // クリーンアップ
        session.shutdown().await.ok();
        engine.stop();
        info!("Cadence セッション終了");

        Ok(())
    }

    /// ゲストとして指定アドレスに接続
    pub async fn run_connect(&self, addr: SocketAddr) -> Result<()> {
        // 1. プラグインの検索とロード
        let plugin_info = self.find_plugin()?;
        let config = self.build_config();

        let sample_rate = config.audio.sample_rate as f64;
        let buffer_size = config.audio.buffer_size;
        let channels = config.audio.channels as usize;

        let (mut processor, note_ctrl, _handle) = plugin_host::load_plugin(
            &plugin_info,
            sample_rate,
            buffer_size,
            buffer_size,
            channels,
        )
        .context("プラグインのロードに失敗")?;

        info!("プラグインをロード: {}", plugin_info.name);

        // 2. AudioEngine を起動
        let mut engine = AudioEngine::new(config.audio.clone());
        engine
            .start(move |buf| {
                processor.process(buf);
            })
            .context("オーディオエンジンの起動に失敗")?;

        info!("オーディオエンジン起動完了");

        // 3. SessionManager でゲスト接続
        let mut session = SessionManager::new(config);
        println!("Cadence: {} に接続中...", addr);

        tokio::select! {
            result = session.join(addr) => {
                match result {
                    Ok(_streamer) => {
                        info!("接続完了。シーケンサーループを開始...");
                        self.run_sequencer_loop(note_ctrl).await?;
                    }
                    Err(e) => {
                        error!("接続に失敗: {e}");
                        return Err(e.into());
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C: セッションを終了します");
            }
        }

        // クリーンアップ
        session.shutdown().await.ok();
        engine.stop();
        info!("Cadence セッション終了");

        Ok(())
    }

    /// シーケンサーメインループ
    ///
    /// ~1ms 間隔で sequencer.update() を呼び、NoteCommand を NoteController に転送する。
    /// 現時点ではネットワーク経由の ControlEvent 受信は未接続。
    /// デモ用にダミーシーケンスで動作確認可能。
    async fn run_sequencer_loop(&self, mut note_ctrl: NoteController) -> Result<()> {
        let mut sequencer = MidiSequencer::new();
        let start = Instant::now();

        info!("シーケンサーループ開始（Ctrl+C で停止）");
        println!("シーケンサー待機中... (ControlEvent::Command でシーケンスを受信予定)");

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Ctrl+C: シーケンサーループを停止");
                    // 全ノートオフ
                    for note in 0..128u8 {
                        note_ctrl.note_off(note);
                    }
                    break;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)) => {
                    let current_time = start.elapsed().as_secs_f64();
                    let commands = sequencer.update(current_time);

                    for cmd in commands {
                        match cmd {
                            NoteCommand::NoteOn { note, velocity } => {
                                note_ctrl.note_on(note, velocity);
                            }
                            NoteCommand::NoteOff { note } => {
                                note_ctrl.note_off(note);
                            }
                            NoteCommand::Stop => {
                                // 全ノートオフ
                                for n in 0..128u8 {
                                    note_ctrl.note_off(n);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_session_new() {
        let session = CadenceSession::new("com.u-he.Diva".into(), 5000);
        assert_eq!(session.plugin_id, "com.u-he.Diva");
        assert_eq!(session.port, 5000);
    }

    #[test]
    fn build_config_uses_port() {
        let session = CadenceSession::new("test".into(), 5555);
        let config = session.build_config();
        assert_eq!(config.network.listen_port, 5555);
        assert_eq!(config.audio.sample_rate, 48_000);
    }
}
