pub mod app;
pub mod renderer;
pub mod state;
pub mod ui;
pub mod visuals;

use state::{AudioMeters, PcmSnapshot, SessionSnapshot};
use std::sync::atomic::AtomicU8;
use std::sync::{Arc, Mutex};

/// HUD に外部データを供給するためのコンテキスト。
///
/// AudioEngine / SessionManager からのリアルタイムデータを HUD に渡す。
/// コンテキストなしで起動するとデモモードで動作する。
pub struct HudContext {
    /// オーディオレベルメーター（AtomicF32 で lock-free 共有）
    pub meters: Arc<AudioMeters>,
    /// セッション状態スナップショット（TripleBuffer 読み取り側）
    pub session: triple_buffer::Output<SessionSnapshot>,
    /// ローカル PCM サンプル（TripleBuffer 読み取り側）
    pub local_pcm: triple_buffer::Output<PcmSnapshot>,
}

// ── Interactive モード（HudBridge）──────────────────

/// スキャン済みプラグインの情報
pub struct PluginEntry {
    pub name: String,
    pub id: String,
    pub vendor: String,
}

/// HUD → App: ユーザー操作の通知
pub enum HudAction {
    Play {
        plugin_index: usize,
        midi_port_index: Option<usize>,
    },
    Stop,
}

/// App → HUD: プラグインロード後のライブデータ
pub struct HudLiveData {
    pub meters: Arc<AudioMeters>,
    pub local_pcm: triple_buffer::Output<PcmSnapshot>,
}

/// App ステータスコード（AtomicU8 で共有）
pub mod app_status {
    pub const READY: u8 = 0;
    pub const LOADING: u8 = 1;
    pub const PLAYING: u8 = 2;
    pub const ERROR: u8 = 3;
}

/// HUD 起動時に渡すブリッジ（双方向通信）
pub struct HudBridge {
    /// 利用可能なプラグイン一覧（起動前にスキャン済み）
    pub plugins: Vec<PluginEntry>,
    /// 利用可能な MIDI ポート一覧
    pub midi_ports: Vec<String>,
    /// HUD → App: ユーザー操作の通知
    pub action_tx: std::sync::mpsc::Sender<HudAction>,
    /// App → HUD: ステータス更新（0=Ready, 1=Loading, 2=Playing, 3=Error）
    pub status: Arc<AtomicU8>,
    /// App → HUD: ステータスメッセージ（エラー詳細等）
    pub status_message: Arc<Mutex<String>>,
    /// App → HUD: ライブデータ（プラグインロード後にセット）
    pub live_data: Arc<Mutex<Option<HudLiveData>>>,
}

/// インタラクティブモードで HUD を起動
pub fn run_interactive(bridge: HudBridge) -> anyhow::Result<()> {
    app::run_with_bridge(bridge)
}
