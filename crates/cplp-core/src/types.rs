use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

/// オーディオパケット: ネットワーク転送の最小単位
///
/// REQ-CORE-003: 生 PCM オーディオデータの送受信
///
/// バイナリフォーマット (spec/03 §7.1):
/// ```text
/// ┌──────────┬──────────┬───────────────────┐
/// │ seq: u32 │ ts: u64  │ PCM data: [f32]   │
/// │ 4 bytes  │ 8 bytes  │ 可変長            │
/// └──────────┴──────────┴───────────────────┘
/// ```
#[derive(Debug, Clone)]
pub struct AudioPacket {
    /// シーケンス番号（パケットロス検知）
    pub seq: u32,
    /// タイムスタンプ（サンプル単位）
    pub timestamp: u64,
    /// PCM データ (f32, interleaved stereo)
    pub pcm_data: Vec<f32>,
}

impl AudioPacket {
    /// バイナリにシリアライズ（ネットワーク送信用）
    ///
    /// フォーマット: seq(4) + timestamp(8) + pcm_data(N×4) = little-endian
    pub fn to_bytes(&self) -> Vec<u8> {
        let pcm_bytes = self.pcm_data.len() * 4;
        let mut buf = Vec::with_capacity(4 + 8 + pcm_bytes);
        buf.extend_from_slice(&self.seq.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        for &sample in &self.pcm_data {
            buf.extend_from_slice(&sample.to_le_bytes());
        }
        buf
    }

    /// バイナリからデシリアライズ（ネットワーク受信用）
    pub fn from_bytes(data: &[u8]) -> Result<Self, CplpError> {
        if data.len() < 12 {
            return Err(CplpError::Network(format!(
                "パケットが短すぎます: {} bytes (最低 12)",
                data.len()
            )));
        }
        if (data.len() - 12) % 4 != 0 {
            return Err(CplpError::Network(format!(
                "PCM データサイズが不正: {} bytes (4の倍数でない)",
                data.len() - 12
            )));
        }

        let seq = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let timestamp = u64::from_le_bytes(data[4..12].try_into().unwrap());

        let sample_count = (data.len() - 12) / 4;
        let mut pcm_data = Vec::with_capacity(sample_count);
        for i in 0..sample_count {
            let offset = 12 + i * 4;
            let sample = f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            pcm_data.push(sample);
        }

        Ok(Self {
            seq,
            timestamp,
            pcm_data,
        })
    }

    /// パケットサイズ（バイト）
    pub fn byte_size(&self) -> usize {
        12 + self.pcm_data.len() * 4
    }
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

// ─── 共有ミキサー型定義 ───────────────────────────────────

/// ピア識別子
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub String);

impl PeerId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// トラック状態（ミキサーの1チャンネル分）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackState {
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub label: String,
    /// Last-write-wins 用タイムスタンプ（各フィールドごと）
    pub last_fader_ts: u64,
    pub last_pan_ts: u64,
    pub last_mute_ts: u64,
    pub last_solo_ts: u64,
}

impl TrackState {
    pub fn new(label: &str) -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            label: label.to_string(),
            last_fader_ts: 0,
            last_pan_ts: 0,
            last_mute_ts: 0,
            last_solo_ts: 0,
        }
    }
}

/// 共有ミキサー状態（各ピアがローカルコピーを保持）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerState {
    pub tracks: HashMap<PeerId, TrackState>,
    pub master_volume: f32,
    pub last_master_ts: u64,
}

impl MixerState {
    pub fn new() -> Self {
        Self {
            tracks: HashMap::new(),
            master_volume: 1.0,
            last_master_ts: 0,
        }
    }

    pub fn add_track(&mut self, peer: PeerId, track: TrackState) {
        self.tracks.insert(peer, track);
    }

    pub fn remove_track(&mut self, peer: &PeerId) {
        self.tracks.remove(peer);
    }

    /// Last-write-wins でフェーダー適用
    pub fn apply_fader(&mut self, peer: &PeerId, volume: f32, ts: u64) {
        if let Some(track) = self.tracks.get_mut(peer) {
            if ts > track.last_fader_ts {
                track.volume = volume.clamp(0.0, 1.0);
                track.last_fader_ts = ts;
            }
        }
    }

    /// Last-write-wins でパン適用
    pub fn apply_pan(&mut self, peer: &PeerId, pan: f32, ts: u64) {
        if let Some(track) = self.tracks.get_mut(peer) {
            if ts > track.last_pan_ts {
                track.pan = pan.clamp(-1.0, 1.0);
                track.last_pan_ts = ts;
            }
        }
    }

    /// Last-write-wins でミュート適用
    pub fn apply_mute(&mut self, peer: &PeerId, mute: bool, ts: u64) {
        if let Some(track) = self.tracks.get_mut(peer) {
            if ts > track.last_mute_ts {
                track.mute = mute;
                track.last_mute_ts = ts;
            }
        }
    }

    /// Last-write-wins でソロ適用
    pub fn apply_solo(&mut self, peer: &PeerId, solo: bool, ts: u64) {
        if let Some(track) = self.tracks.get_mut(peer) {
            if ts > track.last_solo_ts {
                track.solo = solo;
                track.last_solo_ts = ts;
            }
        }
    }

    /// Last-write-wins でマスターボリューム適用
    pub fn apply_master(&mut self, volume: f32, ts: u64) {
        if ts > self.last_master_ts {
            self.master_volume = volume.clamp(0.0, 1.0);
            self.last_master_ts = ts;
        }
    }

    /// ソロがアクティブなトラックがあるか
    pub fn has_solo(&self) -> bool {
        self.tracks.values().any(|t| t.solo)
    }
}

impl Default for MixerState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_id_display_and_eq() {
        let id1 = PeerId::new("player-a");
        let id2 = PeerId::new("player-a");
        let id3 = PeerId::new("player-b");
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.to_string(), "player-a");
    }

    #[test]
    fn test_track_state_default() {
        let track = TrackState::new("Guitar");
        assert_eq!(track.volume, 1.0);
        assert_eq!(track.pan, 0.0);
        assert!(!track.mute);
        assert!(!track.solo);
        assert_eq!(track.label, "Guitar");
    }

    #[test]
    fn test_mixer_state_add_remove_track() {
        let mut mixer = MixerState::new();
        let peer = PeerId::new("player-a");
        mixer.add_track(peer.clone(), TrackState::new("Synth"));
        assert_eq!(mixer.tracks.len(), 1);
        assert!(mixer.tracks.contains_key(&peer));

        mixer.remove_track(&peer);
        assert!(mixer.tracks.is_empty());
    }

    #[test]
    fn test_mixer_state_apply_fader_lww() {
        let mut mixer = MixerState::new();
        let peer = PeerId::new("player-a");
        mixer.add_track(peer.clone(), TrackState::new("Bass"));

        // ts=100 で volume を 0.8 に
        mixer.apply_fader(&peer, 0.8, 100);
        assert_eq!(mixer.tracks[&peer].volume, 0.8);

        // ts=50（古い）→ 無視される
        mixer.apply_fader(&peer, 0.3, 50);
        assert_eq!(mixer.tracks[&peer].volume, 0.8);

        // ts=200（新しい）→ 適用される
        mixer.apply_fader(&peer, 0.5, 200);
        assert_eq!(mixer.tracks[&peer].volume, 0.5);
    }
}
