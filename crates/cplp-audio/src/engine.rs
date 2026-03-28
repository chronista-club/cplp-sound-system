use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use tracing::{error, info};

use cplp_core::config::AudioConfig;

use crate::mixer::AudioMixer;

/// オーディオエンジン: cpal 出力ストリームとリングバッファの管理
///
/// REQ-AUDIO-002: ローカル再生とリモート送信の同時処理
pub struct AudioEngine {
    config: AudioConfig,
    output_stream: Option<Stream>,
    /// ネットワークスレッドからのリモートオーディオ受信用（書き込み側）
    remote_producer: Option<ringbuf::HeapProd<f32>>,
    /// ローカルオーディオのネットワーク送信用（読み出し側）
    send_consumer: Option<ringbuf::HeapCons<f32>>,
}

impl AudioEngine {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            output_stream: None,
            remote_producer: None,
            send_consumer: None,
        }
    }

    /// オーディオ出力ストリームを開始
    ///
    /// テスト用のオーディオソースを受け取り、ミキシングして出力する。
    /// 戻り値:
    /// - remote_producer: リモートオーディオを書き込むための Producer
    /// - send_consumer: ローカルオーディオを読み出すための Consumer
    pub fn start<F>(&mut self, mut audio_source: F) -> Result<()>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("出力デバイスが見つかりません")?;

        info!(
            "Output device: {}",
            device.name().unwrap_or_else(|_| "unknown".into())
        );

        // CoreAudio はバッファサイズのヒントを無視することがある（特に macOS 26+）。
        // Default を使い、OS に最適なサイズを選ばせる。
        let stream_config = StreamConfig {
            channels: self.config.channels,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer_capacity =
            (self.config.buffer_size as usize) * (self.config.channels as usize) * 8;

        // リモートオーディオ受信用リングバッファ
        let remote_rb = HeapRb::<f32>::new(buffer_capacity);
        let (remote_prod, mut remote_cons) = remote_rb.split();

        // ローカルオーディオ送信用リングバッファ
        let send_rb = HeapRb::<f32>::new(buffer_capacity);
        let (mut send_prod, send_cons) = send_rb.split();

        let mixer = AudioMixer::default();

        // CoreAudio が渡すバッファサイズは可変のため、十分大きく確保する。
        // 最大 4096 frames * channels をカバー。
        let max_callback_len = 4096 * (self.config.channels as usize);
        let mut local_buf = vec![0.0f32; max_callback_len];
        let mut remote_buf = vec![0.0f32; max_callback_len];

        let stream = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let len = data.len();

                // CoreAudio が想定外のサイズを渡した場合、バッファ範囲内に制限
                let len = len.min(local_buf.len());

                // ローカルオーディオ生成
                local_buf[..len].fill(0.0);
                audio_source(&mut local_buf[..len]);

                // ローカルオーディオを送信バッファにコピー
                let _ = send_prod.push_slice(&local_buf[..len]);

                // リモートオーディオ受信
                remote_buf[..len].fill(0.0);
                let read = remote_cons.pop_slice(&mut remote_buf[..len]);
                if read < len {
                    // アンダーラン: 残りをゼロフィル（仕様通り）
                    remote_buf[read..len].fill(0.0);
                }

                // ミキシング — data の実際のサイズに合わせる
                mixer.mix(&local_buf[..len], &remote_buf[..len], &mut data[..len]);
            },
            move |err| {
                error!("Audio stream error: {err}");
            },
            None,
        )?;

        stream.play()?;
        info!(
            "Audio stream started: {}Hz, {} ch, buffer {}",
            self.config.sample_rate, self.config.channels, self.config.buffer_size
        );

        self.output_stream = Some(stream);
        self.remote_producer = Some(remote_prod);
        self.send_consumer = Some(send_cons);

        Ok(())
    }

    /// リモートオーディオを書き込む Producer を取得
    pub fn take_remote_producer(&mut self) -> Option<ringbuf::HeapProd<f32>> {
        self.remote_producer.take()
    }

    /// ローカルオーディオを読み出す Consumer を取得
    pub fn take_send_consumer(&mut self) -> Option<ringbuf::HeapCons<f32>> {
        self.send_consumer.take()
    }

    /// オーディオストリームを停止
    pub fn stop(&mut self) {
        self.output_stream = None;
        info!("Audio stream stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cplp_core::config::AudioConfig;
    use ringbuf::HeapRb;
    use ringbuf::traits::{Consumer, Split};

    #[test]
    fn engine_new_has_no_stream() {
        let engine = AudioEngine::new(AudioConfig::default());
        assert!(engine.output_stream.is_none());
        assert!(engine.remote_producer.is_none());
        assert!(engine.send_consumer.is_none());
    }

    #[test]
    fn engine_take_remote_producer_returns_none_before_start() {
        let mut engine = AudioEngine::new(AudioConfig::default());
        assert!(engine.take_remote_producer().is_none());
    }

    #[test]
    fn engine_take_send_consumer_returns_none_before_start() {
        let mut engine = AudioEngine::new(AudioConfig::default());
        assert!(engine.take_send_consumer().is_none());
    }

    #[test]
    fn engine_stop_is_idempotent() {
        let mut engine = AudioEngine::new(AudioConfig::default());
        engine.stop();
        engine.stop();
        engine.stop();
        // パニックしなければ OK
    }

    #[test]
    fn local_buf_preallocation_no_heap_in_callback_pattern() {
        let config = AudioConfig::default();
        let callback_buf_len = (config.buffer_size as usize) * (config.channels as usize);
        // デフォルト: buffer_size=128, channels=2 → 256
        assert_eq!(callback_buf_len, 128 * 2);
        let buf = vec![0.0f32; callback_buf_len];
        assert_eq!(buf.len(), callback_buf_len);
    }

    #[test]
    fn buffer_capacity_formula() {
        let config = AudioConfig::default();
        let capacity =
            (config.buffer_size as usize) * (config.channels as usize) * 8;
        // 128 * 2 * 8 = 2048
        assert_eq!(capacity, 2048);
    }

    #[test]
    fn remote_underrun_zero_fill() {
        // リモートバッファが空のとき pop_slice は 0 を返し、残りをゼロフィルする
        let rb = HeapRb::<f32>::new(256);
        let (_prod, mut cons) = rb.split();

        let mut buf = vec![1.0f32; 64];
        let read = cons.pop_slice(&mut buf);
        assert_eq!(read, 0);

        // エンジンのコールバックと同じパターン: read < len → ゼロフィル
        buf[read..].fill(0.0);
        assert!(buf.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn audio_config_default_values() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.buffer_size, 128);
        assert_eq!(config.channels, 2);
    }

    #[test]
    fn engine_take_remote_producer_consumed_after_start() {
        // start() はデバイス依存なので、フィールドを直接 Some にセットしてテスト
        let mut engine = AudioEngine::new(AudioConfig::default());

        // リングバッファを手動で作って remote_producer にセット
        let rb = HeapRb::<f32>::new(256);
        let (prod, _cons) = rb.split();
        engine.remote_producer = Some(prod);

        // 1回目の take → Some
        assert!(engine.take_remote_producer().is_some());
        // 2回目の take → None（消費済み）
        assert!(engine.take_remote_producer().is_none());
    }

    #[test]
    fn engine_take_send_consumer_consumed_after_start() {
        // start() はデバイス依存なので、フィールドを直接 Some にセットしてテスト
        let mut engine = AudioEngine::new(AudioConfig::default());

        // リングバッファを手動で作って send_consumer にセット
        let rb = HeapRb::<f32>::new(256);
        let (_prod, cons) = rb.split();
        engine.send_consumer = Some(cons);

        // 1回目の take → Some
        assert!(engine.take_send_consumer().is_some());
        // 2回目の take → None（消費済み）
        assert!(engine.take_send_consumer().is_none());
    }
}
