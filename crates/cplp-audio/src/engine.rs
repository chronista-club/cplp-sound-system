use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
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

        let stream_config = StreamConfig {
            channels: self.config.channels,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Fixed(self.config.buffer_size),
        };

        let buffer_capacity = (self.config.buffer_size as usize) * (self.config.channels as usize) * 8;

        // リモートオーディオ受信用リングバッファ
        let remote_rb = HeapRb::<f32>::new(buffer_capacity);
        let (remote_prod, mut remote_cons) = remote_rb.split();

        // ローカルオーディオ送信用リングバッファ
        let send_rb = HeapRb::<f32>::new(buffer_capacity);
        let (mut send_prod, send_cons) = send_rb.split();

        let mixer = AudioMixer::default();
        let stream = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // ローカルオーディオ生成
                let mut local_buf = vec![0.0f32; data.len()];
                audio_source(&mut local_buf);

                // ローカルオーディオを送信バッファにコピー
                let _ = send_prod.push_slice(&local_buf);

                // リモートオーディオ受信
                let mut remote_buf = vec![0.0f32; data.len()];
                let read = remote_cons.pop_slice(&mut remote_buf);
                if read < remote_buf.len() {
                    // アンダーラン: 残りをゼロフィル（仕様通り）
                    remote_buf[read..].fill(0.0);
                }

                // ミキシング
                mixer.mix(&local_buf, &remote_buf, data);
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
