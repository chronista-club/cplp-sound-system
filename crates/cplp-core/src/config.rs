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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_config_default_values() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.buffer_size, 128);
        assert_eq!(config.channels, 2);
    }

    #[test]
    fn network_config_default_values() {
        let config = NetworkConfig::default();
        assert_eq!(config.listen_port, 5000);
        assert_eq!(config.jitter_buffer_depth, 2);
    }

    #[test]
    fn app_config_contains_defaults() {
        let config = AppConfig::default();
        // AudioConfig のデフォルト値が含まれている
        assert_eq!(config.audio.sample_rate, 48_000);
        assert_eq!(config.audio.buffer_size, 128);
        assert_eq!(config.audio.channels, 2);
        // NetworkConfig のデフォルト値が含まれている
        assert_eq!(config.network.listen_port, 5000);
        assert_eq!(config.network.jitter_buffer_depth, 2);
    }
}
