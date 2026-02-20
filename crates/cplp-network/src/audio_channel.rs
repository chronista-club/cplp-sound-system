//! AudioStreamer: PCM オーディオの送受信
//!
//! REQ-CORE-001: フルデュプレックスオーディオストリーミング
//! REQ-CORE-003: 生 PCM オーディオデータの送受信
//!
//! Unison の raw bytes チャネルを使って AudioPacket を送受信する。
//! オーディオスレッド（cpal callback）とネットワークスレッド間は
//! ringbuf で lock-free に接続する。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use cplp_core::{AudioPacket, CplpError, PeerId};
use tokio::sync::mpsc;

/// AudioStreamer: ネットワーク経由のオーディオ送受信
///
/// 送信側: オーディオスレッドが ringbuf に PCM を書く → ネットワークスレッドが読んで送信
/// 受信側: ネットワークスレッドが受信 → ジッタバッファ → オーディオスレッドが読む
pub struct AudioStreamer {
    /// 送信用シーケンス番号
    send_seq: AtomicU32,
    /// 送信パケットキュー
    send_tx: mpsc::Sender<AudioPacket>,
    send_rx: Option<mpsc::Receiver<AudioPacket>>,
    /// 受信パケットキュー
    recv_tx: mpsc::Sender<AudioPacket>,
    recv_rx: Option<mpsc::Receiver<AudioPacket>>,
    /// N ピア受信トラック（ピアごとの受信キュー送信側）
    peer_recv_txs: HashMap<PeerId, mpsc::Sender<AudioPacket>>,
    /// N ピア受信トラック（ピアごとの受信キュー受信側）
    peer_recv_rxs: HashMap<PeerId, mpsc::Receiver<AudioPacket>>,
}

impl AudioStreamer {
    pub fn new() -> Self {
        // 送信キュー: オーディオスレッド → ネットワークスレッド
        let (send_tx, send_rx) = mpsc::channel(64);
        // 受信キュー: ネットワークスレッド → ジッタバッファ → オーディオスレッド
        let (recv_tx, recv_rx) = mpsc::channel(64);

        Self {
            send_seq: AtomicU32::new(0),
            send_tx,
            send_rx: Some(send_rx),
            recv_tx,
            recv_rx: Some(recv_rx),
            peer_recv_txs: HashMap::new(),
            peer_recv_rxs: HashMap::new(),
        }
    }

    /// 送信キューの Sender を取得（オーディオスレッドで使う）
    pub fn send_handle(&self) -> AudioSendHandle<'_> {
        AudioSendHandle {
            tx: self.send_tx.clone(),
            seq: &self.send_seq,
        }
    }

    /// 受信キューの Receiver を取得（一度だけ呼べる）
    pub fn take_recv_rx(&mut self) -> Option<mpsc::Receiver<AudioPacket>> {
        self.recv_rx.take()
    }

    /// 送信キューの Receiver を取得（ネットワークスレッドで使う、一度だけ）
    pub fn take_send_rx(&mut self) -> Option<mpsc::Receiver<AudioPacket>> {
        self.send_rx.take()
    }

    /// 受信パケットをキューに追加（ネットワークスレッドから呼ぶ）
    pub async fn push_received(&self, packet: AudioPacket) -> Result<(), CplpError> {
        self.recv_tx
            .send(packet)
            .await
            .map_err(|_| CplpError::Network("受信キューが閉じています".to_string()))
    }

    /// ピアの受信トラックを追加
    pub fn add_peer_track(&mut self, peer_id: PeerId) {
        let (tx, rx) = mpsc::channel(64);
        self.peer_recv_txs.insert(peer_id.clone(), tx);
        self.peer_recv_rxs.insert(peer_id, rx);
    }

    /// ピアの受信トラックを削除
    pub fn remove_peer_track(&mut self, peer_id: &PeerId) {
        self.peer_recv_txs.remove(peer_id);
        self.peer_recv_rxs.remove(peer_id);
    }

    /// ピアが存在するか
    pub fn has_peer(&self, peer_id: &PeerId) -> bool {
        self.peer_recv_txs.contains_key(peer_id)
    }

    /// ピアの受信キューにパケットを追加
    pub async fn push_peer_received(&self, peer_id: &PeerId, packet: AudioPacket) -> Result<(), CplpError> {
        let tx = self.peer_recv_txs.get(peer_id).ok_or_else(|| {
            CplpError::Network(format!("Unknown peer: {}", peer_id))
        })?;
        tx.send(packet).await.map_err(|_| {
            CplpError::Network(format!("受信キューが閉じています: {}", peer_id))
        })
    }

    /// ピアの受信キュー Receiver を取得（一度だけ）
    pub fn take_peer_recv_rx(&mut self, peer_id: &PeerId) -> Option<mpsc::Receiver<AudioPacket>> {
        self.peer_recv_rxs.remove(peer_id)
    }

    /// ネットワーク送信ループを起動
    ///
    /// send_rx からパケットを読み、Unison の raw bytes チャネルで送信する。
    /// Unison API 確定後に channel パラメータを追加。
    pub async fn run_send_loop(
        mut send_rx: mpsc::Receiver<AudioPacket>,
        // TODO: raw_channel: UnisonRawChannel,
    ) -> Result<(), CplpError> {
        while let Some(packet) = send_rx.recv().await {
            let bytes = packet.to_bytes();
            // TODO: raw_channel.send_raw(&bytes).await?;
            tracing::trace!("Sent audio packet seq={} ({} bytes)", packet.seq, bytes.len());
        }
        Ok(())
    }

    /// ネットワーク受信ループを起動
    ///
    /// Unison の raw bytes チャネルから受信し、recv_tx に流す。
    pub async fn run_recv_loop(
        recv_tx: mpsc::Sender<AudioPacket>,
        // TODO: raw_channel: UnisonRawChannel,
    ) -> Result<(), CplpError> {
        // TODO: Unison raw bytes チャネルから受信ループ
        // loop {
        //     let bytes = raw_channel.recv_raw().await?;
        //     let packet = AudioPacket::from_bytes(&bytes)?;
        //     recv_tx.send(packet).await.map_err(|_| ...)?;
        // }
        let _ = recv_tx;
        Ok(())
    }
}

/// オーディオスレッドから送信キューにパケットを投入するハンドル
pub struct AudioSendHandle<'a> {
    tx: mpsc::Sender<AudioPacket>,
    seq: &'a AtomicU32,
}

impl AudioSendHandle<'_> {
    /// PCM データをパケット化して送信キューに投入
    ///
    /// cpal callback から呼ばれる。try_send で non-blocking。
    pub fn send_pcm(&self, pcm_data: &[f32], timestamp: u64) -> Result<(), CplpError> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let packet = AudioPacket {
            seq,
            timestamp,
            pcm_data: pcm_data.to_vec(),
        };

        self.tx.try_send(packet).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                tracing::warn!("Audio send queue full, dropping packet seq={}", seq);
                CplpError::Network("送信キューが満杯".to_string())
            }
            mpsc::error::TrySendError::Closed(_) => {
                CplpError::Network("送信キューが閉じています".to_string())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audio_packet_roundtrip() {
        let original = AudioPacket {
            seq: 42,
            timestamp: 128000,
            pcm_data: vec![0.5, -0.3, 0.0, 1.0],
        };

        let bytes = original.to_bytes();
        assert_eq!(bytes.len(), 12 + 4 * 4); // header + 4 samples

        let decoded = AudioPacket::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.timestamp, 128000);
        assert_eq!(decoded.pcm_data, vec![0.5, -0.3, 0.0, 1.0]);
    }

    #[tokio::test]
    async fn test_streamer_send_receive() {
        let mut streamer = AudioStreamer::new();
        let handle = streamer.send_handle();
        let mut recv_rx = streamer.take_recv_rx().unwrap();

        // 受信側にパケットを直接プッシュ
        let packet = AudioPacket {
            seq: 0,
            timestamp: 0,
            pcm_data: vec![0.1, 0.2],
        };
        streamer.push_received(packet).await.unwrap();

        // 受信できることを確認
        let received = recv_rx.recv().await.unwrap();
        assert_eq!(received.seq, 0);
        assert_eq!(received.pcm_data, vec![0.1, 0.2]);
    }

    #[tokio::test]
    async fn test_multi_peer_streamer() {
        let mut streamer = AudioStreamer::new();
        let peer_a = PeerId::new("peer-a");
        let peer_b = PeerId::new("peer-b");

        streamer.add_peer_track(peer_a.clone());
        streamer.add_peer_track(peer_b.clone());

        let packet_a = AudioPacket { seq: 0, timestamp: 0, pcm_data: vec![0.5, 0.5] };
        let packet_b = AudioPacket { seq: 0, timestamp: 0, pcm_data: vec![0.3, 0.3] };

        streamer.push_peer_received(&peer_a, packet_a).await.unwrap();
        streamer.push_peer_received(&peer_b, packet_b).await.unwrap();

        let mut rx_a = streamer.take_peer_recv_rx(&peer_a).unwrap();
        let mut rx_b = streamer.take_peer_recv_rx(&peer_b).unwrap();

        let received_a = rx_a.recv().await.unwrap();
        assert_eq!(received_a.pcm_data, vec![0.5, 0.5]);

        let received_b = rx_b.recv().await.unwrap();
        assert_eq!(received_b.pcm_data, vec![0.3, 0.3]);
    }

    #[tokio::test]
    async fn test_remove_peer_track() {
        let mut streamer = AudioStreamer::new();
        let peer_a = PeerId::new("peer-a");
        streamer.add_peer_track(peer_a.clone());
        assert!(streamer.has_peer(&peer_a));

        streamer.remove_peer_track(&peer_a);
        assert!(!streamer.has_peer(&peer_a));
    }
}
