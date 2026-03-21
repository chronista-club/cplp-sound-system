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

// ─── 信号フローグラフ型定義 ──────────────────────────────

/// 信号フローグラフのノード種別
#[derive(Debug, Clone)]
pub enum AudioNodeKind {
    MidiInput,
    Synth { plugin_name: String },
    BeatMachine,
    Looper,
    Mixer,
    NetworkSend,
    NetworkRecv,
    AudioOutput,
}

impl AudioNodeKind {
    /// 表示用ラベル
    pub fn label(&self) -> String {
        match self {
            Self::MidiInput => "MIDI In".into(),
            Self::Synth { plugin_name } => {
                if plugin_name.is_empty() {
                    "Synth".into()
                } else {
                    plugin_name.clone()
                }
            }
            Self::BeatMachine => "Beat".into(),
            Self::Looper => "Looper".into(),
            Self::Mixer => "Mixer".into(),
            Self::NetworkSend => "Net Send".into(),
            Self::NetworkRecv => "Net Recv".into(),
            Self::AudioOutput => "Output".into(),
        }
    }
}

/// ノードの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeActivity {
    Inactive,
    Active,
    Error,
}

/// グラフ内の 1 ノード
#[derive(Debug, Clone)]
pub struct AudioNode {
    pub kind: AudioNodeKind,
    pub activity: NodeActivity,
    /// 0.0–1.0 信号レベル
    pub level: f32,
}

/// ノード間の接続
#[derive(Debug, Clone)]
pub struct AudioEdge {
    /// nodes[] のインデックス
    pub from: usize,
    /// nodes[] のインデックス
    pub to: usize,
    /// 接続上の信号レベル（色の強さ）
    pub level: f32,
}

/// 信号フローグラフ全体の状態
#[derive(Debug, Clone, Default)]
pub struct AudioGraphState {
    pub nodes: Vec<AudioNode>,
    pub edges: Vec<AudioEdge>,
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

    // ─── CPS-25 Phase 2a: LWW テスト ───────────────────────

    #[test]
    fn mixer_state_lww_fader_old_ts_ignored() {
        let mut mixer = MixerState::new();
        let peer = PeerId::new("alice");
        mixer.add_track(peer.clone(), TrackState::new("Vocal"));
        mixer.apply_fader(&peer, 0.7, 100);
        // 古い ts → 無視
        mixer.apply_fader(&peer, 0.2, 50);
        assert_eq!(mixer.tracks[&peer].volume, 0.7);
    }

    #[test]
    fn mixer_state_lww_fader_equal_ts_ignored() {
        let mut mixer = MixerState::new();
        let peer = PeerId::new("bob");
        mixer.add_track(peer.clone(), TrackState::new("Guitar"));
        mixer.apply_fader(&peer, 0.6, 100);
        // 同値 ts → > のみ許可なので無視
        mixer.apply_fader(&peer, 0.9, 100);
        assert_eq!(mixer.tracks[&peer].volume, 0.6);
    }

    #[test]
    fn mixer_state_lww_pan_concurrent_wins() {
        let mut mixer = MixerState::new();
        let peer = PeerId::new("carol");
        mixer.add_track(peer.clone(), TrackState::new("Keys"));
        mixer.apply_pan(&peer, -0.5, 10);
        assert_eq!(mixer.tracks[&peer].pan, -0.5);
        // 新しい ts が上書き
        mixer.apply_pan(&peer, 0.8, 20);
        assert_eq!(mixer.tracks[&peer].pan, 0.8);
    }

    #[test]
    fn has_solo_with_multiple_solos() {
        let mut mixer = MixerState::new();
        let p1 = PeerId::new("p1");
        let p2 = PeerId::new("p2");
        mixer.add_track(p1.clone(), TrackState::new("A"));
        mixer.add_track(p2.clone(), TrackState::new("B"));
        mixer.apply_solo(&p1, true, 1);
        mixer.apply_solo(&p2, true, 1);
        assert!(mixer.has_solo());
    }

    #[test]
    fn has_solo_false_when_all_off() {
        let mut mixer = MixerState::new();
        let p1 = PeerId::new("p1");
        let p2 = PeerId::new("p2");
        mixer.add_track(p1.clone(), TrackState::new("A"));
        mixer.add_track(p2.clone(), TrackState::new("B"));
        mixer.apply_solo(&p1, true, 1);
        mixer.apply_solo(&p2, true, 1);
        // 全解除
        mixer.apply_solo(&p1, false, 2);
        mixer.apply_solo(&p2, false, 2);
        assert!(!mixer.has_solo());
    }

    #[test]
    fn remove_nonexistent_track_noop() {
        let mut mixer = MixerState::new();
        let ghost = PeerId::new("ghost");
        // パニックしないことを検証
        mixer.remove_track(&ghost);
        assert!(mixer.tracks.is_empty());
    }

    #[test]
    fn peer_id_display_format() {
        let id = PeerId::new("alice");
        assert_eq!(format!("{}", id), "alice");
    }

    #[test]
    fn peer_id_hash_equality() {
        use std::collections::HashMap;
        let a = PeerId::new("same-key");
        let b = PeerId::new("same-key");
        let mut map = HashMap::new();
        map.insert(a, 42);
        assert_eq!(map[&b], 42);
    }

    #[test]
    fn audio_packet_roundtrip() {
        let orig = AudioPacket {
            seq: 42,
            timestamp: 12345678,
            pcm_data: vec![0.1, -0.5, 0.9, 0.0],
        };
        let bytes = orig.to_bytes();
        let restored = AudioPacket::from_bytes(&bytes).unwrap();
        assert_eq!(restored.seq, orig.seq);
        assert_eq!(restored.timestamp, orig.timestamp);
        assert_eq!(restored.pcm_data.len(), orig.pcm_data.len());
        for (a, b) in restored.pcm_data.iter().zip(orig.pcm_data.iter()) {
            assert!((a - b).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn audio_packet_empty_pcm() {
        let pkt = AudioPacket {
            seq: 0,
            timestamp: 0,
            pcm_data: vec![],
        };
        let bytes = pkt.to_bytes();
        assert_eq!(bytes.len(), 12);
        let restored = AudioPacket::from_bytes(&bytes).unwrap();
        assert!(restored.pcm_data.is_empty());
    }

    #[test]
    fn audio_packet_too_short_error() {
        // 11 バイト以下で Err
        let short = vec![0u8; 11];
        assert!(AudioPacket::from_bytes(&short).is_err());
        // 0 バイトでも Err
        assert!(AudioPacket::from_bytes(&[]).is_err());
    }

    #[test]
    fn audio_packet_misaligned_size_error() {
        // 12 + 3 = 15 バイト → (15 - 12) % 4 != 0
        let bad = vec![0u8; 15];
        assert!(AudioPacket::from_bytes(&bad).is_err());
        // 12 + 1 = 13 バイト
        let bad2 = vec![0u8; 13];
        assert!(AudioPacket::from_bytes(&bad2).is_err());
    }

    #[test]
    fn audio_packet_byte_size_formula() {
        let pkt = AudioPacket {
            seq: 1,
            timestamp: 2,
            pcm_data: vec![0.0; 10],
        };
        assert_eq!(pkt.byte_size(), 12 + 10 * 4);
        let empty = AudioPacket {
            seq: 0,
            timestamp: 0,
            pcm_data: vec![],
        };
        assert_eq!(empty.byte_size(), 12);
    }
}
