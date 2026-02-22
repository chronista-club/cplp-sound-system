pub mod app;
pub mod renderer;
pub mod state;
pub mod ui;
pub mod visuals;

use state::{AudioMeters, PcmSnapshot, SessionSnapshot};
use std::sync::Arc;

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
