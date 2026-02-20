use thiserror::Error;

/// オーディオパケット: ネットワーク転送の最小単位
///
/// REQ-CORE-003: 生 PCM オーディオデータの送受信
#[derive(Debug, Clone)]
pub struct AudioPacket {
    /// シーケンス番号（パケットロス検知）
    pub seq: u32,
    /// タイムスタンプ（サンプル単位）
    pub timestamp: u64,
    /// PCM データ (f32, interleaved stereo)
    pub pcm_data: Vec<f32>,
}

/// ピアの状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerStatus {
    Connecting,
    Connected,
    SessionActive,
    Disconnecting,
    Disconnected,
}

/// cplp-sound-system 共通エラー型
#[derive(Debug, Error)]
pub enum CplpError {
    #[error("audio error: {0}")]
    Audio(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("session error: {0}")]
    Session(String),

    #[error("plugin error: {0}")]
    Plugin(String),
}
