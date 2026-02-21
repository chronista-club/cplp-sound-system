use serde::{Deserialize, Serialize};

/// アプリケーション全体の設定
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub audio: AudioConfig,
    pub network: NetworkConfig,
}

/// オーディオ設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// サンプルレート (Hz)
    pub sample_rate: u32,
    /// バッファサイズ (samples)
    pub buffer_size: u32,
    /// チャネル数
    pub channels: u16,
}

/// ネットワーク設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// リッスンポート
    pub listen_port: u16,
    /// ジッタバッファ深度 (バッファ数)
    pub jitter_buffer_depth: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            buffer_size: 128,
            channels: 2,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_port: 5000,
            jitter_buffer_depth: 2,
        }
    }
}
