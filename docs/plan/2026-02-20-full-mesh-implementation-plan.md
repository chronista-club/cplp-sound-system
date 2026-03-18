# Full Mesh P2P + 共有ミキサー + ロビーサーバー 実装計画

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** cplp-sound-system を2人P2Pから最大5人フルメッシュP2P + 共有ミキサー + Axumロビーサーバーに拡張する

**Architecture:** フルメッシュP2Pで各ピアが全員と直接接続。共有ミキサー状態はcontrolチャネル経由でレプリケーション（Last-write-wins）。ロビーサーバー（Axum+SurrealDB）がOAuth認証・グループ管理・シグナリングを担当し、オーディオは一切経由しない。

**Tech Stack:** Rust (Unison Protocol, Axum, SurrealDB SDK, tokio), QUIC (via quinn), serde_json

**設計ドキュメント:** `docs/plans/2026-02-20-full-mesh-shared-mixer-design.md`

---

## Phase 1: フルメッシュ P2P（Unison 統合）

### Task 1: PeerId + MixerState + TrackState を cplp-core に追加

**Files:**
- Modify: `crates/cplp-core/src/types.rs`
- Modify: `crates/cplp-core/src/lib.rs`

**Step 1: Write the failing test**

`crates/cplp-core/src/types.rs` の末尾テストモジュールに追加:

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p cplp-core`
Expected: FAIL - PeerId, TrackState, MixerState not defined

**Step 3: Write minimal implementation**

`crates/cplp-core/src/types.rs` に以下を追加:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

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
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p cplp-core`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add crates/cplp-core/src/types.rs
git commit -m "feat: PeerId, TrackState, MixerState をcplp-coreに追加

REQ-MIXER-001: 共有ミキサー状態の型定義
- PeerId: ピア識別子（String wrapper）
- TrackState: Volume/Pan/Mute/Solo + LWWタイムスタンプ
- MixerState: HashMap<PeerId, TrackState> + マスターボリューム
- Last-write-wins 競合解決メソッド群"
```

---

### Task 2: ControlEvent を拡張（ミキサー + セッション管理イベント）

**Files:**
- Modify: `crates/cplp-network/src/control.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cplp_core::PeerId;

    #[test]
    fn test_control_event_serialization() {
        let event = ControlEvent::FaderChange {
            track: PeerId::new("player-a"),
            volume: 0.8,
            ts: 12345,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("FaderChange"));
        assert!(json.contains("player-a"));

        let decoded: ControlEvent = serde_json::from_str(&json).unwrap();
        if let ControlEvent::FaderChange { track, volume, ts } = decoded {
            assert_eq!(track, PeerId::new("player-a"));
            assert!((volume - 0.8).abs() < f32::EPSILON);
            assert_eq!(ts, 12345);
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_mixer_sync_serialization() {
        let mut state = MixerState::new();
        state.add_track(PeerId::new("p1"), TrackState::new("Synth"));
        let event = ControlEvent::MixerSync { state };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: ControlEvent = serde_json::from_str(&json).unwrap();
        if let ControlEvent::MixerSync { state } = decoded {
            assert_eq!(state.tracks.len(), 1);
        } else {
            panic!("Wrong variant");
        }
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p cplp-network`
Expected: FAIL - ControlEvent variants not defined

**Step 3: Rewrite control.rs**

`crates/cplp-network/src/control.rs` を書き換え:

```rust
//! ControlHandler: ミキサー制御 + セッション管理
//!
//! REQ-NET-003: QUIC 上の独立チャネルによるオーディオ/コントロール分離
//! REQ-MIXER-001: 共有ミキサー状態の同期

use std::collections::HashMap;
use std::net::SocketAddr;

use cplp_core::{CplpError, MixerState, PeerId, TrackState};
use serde::{Deserialize, Serialize};

/// control チャネルイベント（全ピア間で送受信）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ControlEvent {
    // ── ミキサー操作 ──
    FaderChange { track: PeerId, volume: f32, ts: u64 },
    PanChange { track: PeerId, pan: f32, ts: u64 },
    MuteToggle { track: PeerId, mute: bool, ts: u64 },
    SoloToggle { track: PeerId, solo: bool, ts: u64 },
    MasterVol { volume: f32, ts: u64 },

    // ── セッション管理 ──
    PeerJoined { peer: PeerId, addr: SocketAddr, label: String },
    PeerLeft { peer: PeerId },
    /// 途中参加者へのミキサー全状態同期
    MixerSync { state: MixerState },

    // ── モニタリング ──
    LatencyReport { rtt_us: u64, jitter_us: u64 },

    // ── プラグイン情報 ──
    PluginInfo { name: String, vendor: String },
    PluginChanged { name: String, vendor: String },
}

/// ControlHandler: control チャネルの処理
///
/// 各ピアとの control チャネルを管理し、
/// ミキサーイベントの送受信と MixerState の更新を行う。
pub struct ControlHandler {
    /// 共有ミキサー状態
    mixer_state: MixerState,
}

impl ControlHandler {
    pub fn new() -> Self {
        Self {
            mixer_state: MixerState::new(),
        }
    }

    /// ミキサー状態の参照を取得
    pub fn mixer_state(&self) -> &MixerState {
        &self.mixer_state
    }

    /// ミキサー状態の可変参照を取得
    pub fn mixer_state_mut(&mut self) -> &mut MixerState {
        &mut self.mixer_state
    }

    /// 受信した ControlEvent をローカルの MixerState に適用
    pub fn apply_event(&mut self, event: &ControlEvent) {
        match event {
            ControlEvent::FaderChange { track, volume, ts } => {
                self.mixer_state.apply_fader(track, *volume, *ts);
            }
            ControlEvent::PanChange { track, pan, ts } => {
                self.mixer_state.apply_pan(track, *pan, *ts);
            }
            ControlEvent::MuteToggle { track, mute, ts } => {
                self.mixer_state.apply_mute(track, *mute, *ts);
            }
            ControlEvent::SoloToggle { track, solo, ts } => {
                self.mixer_state.apply_solo(track, *solo, *ts);
            }
            ControlEvent::MasterVol { volume, ts } => {
                self.mixer_state.apply_master(*volume, *ts);
            }
            ControlEvent::PeerJoined { peer, label, .. } => {
                self.mixer_state.add_track(peer.clone(), TrackState::new(label));
            }
            ControlEvent::PeerLeft { peer } => {
                self.mixer_state.remove_track(peer);
            }
            ControlEvent::MixerSync { state } => {
                self.mixer_state = state.clone();
            }
            _ => {} // LatencyReport, PluginInfo, PluginChanged はミキサーに影響しない
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p cplp-network`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add crates/cplp-network/src/control.rs
git commit -m "feat: ControlEvent を共有ミキサー対応に拡張

REQ-MIXER-001: ミキサー操作イベント（Fader/Pan/Mute/Solo/Master）
REQ-SESSION-001: セッション管理イベント（PeerJoined/Left/MixerSync）
ControlHandler.apply_event() で受信イベントをローカルMixerStateに適用"
```

---

### Task 3: AudioMixer を N トラック対応に拡張

**Files:**
- Modify: `crates/cplp-audio/src/mixer.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cplp_core::{MixerState, PeerId, TrackState};

    #[test]
    fn mix_basic() {
        // 既存テスト維持（後方互換）
        let mixer = AudioMixer::default();
        let local = [0.5, -0.3];
        let remote = [0.3, 0.2];
        let mut output = [0.0; 2];
        mixer.mix(&local, &remote, &mut output);
        assert!((output[0] - 0.8).abs() < f32::EPSILON);
        assert!((output[1] - -0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn mix_clamps() {
        let mixer = AudioMixer { local_gain: 1.0, remote_gain: 1.0 };
        let local = [0.8];
        let remote = [0.8];
        let mut output = [0.0; 1];
        mixer.mix(&local, &remote, &mut output);
        assert_eq!(output[0], 1.0);
    }

    #[test]
    fn mix_multi_tracks_with_mixer_state() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");
        let peer_b = PeerId::new("peer-b");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.add_track(peer_b.clone(), TrackState::new("Bass"));

        // peer_b を 0.5 に
        state.apply_fader(&peer_b, 0.5, 1);

        let local_buf = vec![0.4, 0.4]; // stereo frame
        let mut remote_bufs = std::collections::HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.2, 0.2]);
        remote_bufs.insert(peer_b.clone(), vec![0.6, 0.6]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // local(0.4*1.0) + peer_a(0.2*1.0) + peer_b(0.6*0.5) = 0.4 + 0.2 + 0.3 = 0.9
        assert!((output[0] - 0.9).abs() < 0.001);
    }

    #[test]
    fn mix_with_mute() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.apply_mute(&peer_a, true, 1);

        let local_buf = vec![0.5];
        let mut remote_bufs = std::collections::HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.5]);

        let mut output = vec![0.0; 1];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // peer_a はミュート → local のみ
        assert!((output[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn mix_with_solo() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");
        let peer_b = PeerId::new("peer-b");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.add_track(peer_b.clone(), TrackState::new("Bass"));
        state.apply_solo(&peer_a, true, 1);

        let local_buf = vec![0.3];
        let mut remote_bufs = std::collections::HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.4]);
        remote_bufs.insert(peer_b.clone(), vec![0.5]);

        let mut output = vec![0.0; 1];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // solo = peer_a のみ → 0.4
        assert!((output[0] - 0.4).abs() < 0.001);
    }

    #[test]
    fn mix_with_pan_stereo() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        let mut track = TrackState::new("Guitar");
        // pan = 1.0 (hard right)
        state.add_track(peer_a.clone(), track);
        state.apply_pan(&peer_a, 1.0, 1);

        let local_buf = vec![0.0, 0.0]; // silent local
        let mut remote_bufs = std::collections::HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.8, 0.8]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // hard right: L = 0.0, R = 0.8
        assert!(output[0].abs() < 0.001); // L should be ~0
        assert!((output[1] - 0.8).abs() < 0.001); // R should be 0.8
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p cplp-audio -- mixer`
Expected: FAIL - `mix_with_state` not defined

**Step 3: Add `mix_with_state` function**

`crates/cplp-audio/src/mixer.rs` に追加（既存の `AudioMixer` は残す）:

```rust
use std::collections::HashMap;
use cplp_core::{MixerState, PeerId};

/// N トラック対応ミキシング（MixerState 適用）
///
/// stereo interleaved フォーマット前提（偶数インデックス=L、奇数=R）。
/// パンは equal-power panning: L = cos(θ), R = sin(θ) where θ = (pan+1)/2 * π/2
pub fn mix_with_state(
    local_id: &PeerId,
    state: &MixerState,
    local_buf: &[f32],
    remote_bufs: &HashMap<PeerId, Vec<f32>>,
    output: &mut [f32],
) {
    let has_solo = state.has_solo();
    let channels = 2; // stereo

    // 出力をゼロクリア
    output.iter_mut().for_each(|s| *s = 0.0);

    // ローカルトラック
    if let Some(track) = state.tracks.get(local_id) {
        if should_output(track, has_solo) {
            add_track(local_buf, track, channels, output);
        }
    }

    // リモートトラック
    for (peer_id, buf) in remote_bufs {
        if let Some(track) = state.tracks.get(peer_id) {
            if should_output(track, has_solo) {
                add_track(buf, track, channels, output);
            }
        }
    }

    // マスターボリューム + クランプ
    let master = state.master_volume;
    for sample in output.iter_mut() {
        *sample = (*sample * master).clamp(-1.0, 1.0);
    }
}

/// トラックが出力されるべきか判定
fn should_output(track: &cplp_core::TrackState, has_solo: bool) -> bool {
    if track.mute {
        return false;
    }
    if has_solo && !track.solo {
        return false;
    }
    true
}

/// トラックを出力バッファに加算（volume + pan 適用）
fn add_track(
    src: &[f32],
    track: &cplp_core::TrackState,
    channels: usize,
    output: &mut [f32],
) {
    // Equal-power panning: θ = (pan + 1) / 2 * π/2
    let theta = (track.pan + 1.0) / 2.0 * std::f32::consts::FRAC_PI_2;
    let gain_l = theta.cos() * track.volume;
    let gain_r = theta.sin() * track.volume;

    for (i, frame) in src.chunks(channels).enumerate() {
        let out_idx = i * channels;
        if out_idx + 1 < output.len() && frame.len() >= 2 {
            output[out_idx] += frame[0] * gain_l;
            output[out_idx + 1] += frame[1] * gain_r;
        } else if out_idx < output.len() && !frame.is_empty() {
            // mono fallback
            output[out_idx] += frame[0] * track.volume;
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p cplp-audio -- mixer`
Expected: ALL PASS (既存2テスト + 新規5テスト)

**Step 5: Commit**

```bash
git add crates/cplp-audio/src/mixer.rs
git commit -m "feat: N トラック対応ミキシング (mix_with_state)

REQ-MIXER-001: MixerState を適用した N トラックミキシング
- Volume/Pan/Mute/Solo 対応
- Equal-power panning (cos/sin)
- Solo モード: Solo トラックのみ出力
- 既存 AudioMixer は後方互換で維持"
```

---

### Task 4: Unison 依存を追加してビルド確認

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/cplp-network/Cargo.toml`

**Step 1: Add unison dependency to workspace**

`Cargo.toml` の `[workspace.dependencies]` セクションに追加:

```toml
# P2P 通信
unison = { git = "https://github.com/nicories/unison", tag = "v0.3.0" }
```

**注意**: Unison リポジトリの正確な URL を確認すること。ローカルの場合:

```toml
unison = { path = "../unison/crates/unison-protocol" }
```

`crates/cplp-network/Cargo.toml` に追加:

```toml
unison.workspace = true
```

**Step 2: Build to verify dependency resolves**

Run: `cargo check -p cplp-network`
Expected: compilation succeeds (warnings OK)

**Step 3: Commit**

```bash
git add Cargo.toml crates/cplp-network/Cargo.toml
git commit -m "chore: Unison Protocol 依存を追加"
```

---

### Task 5: P2pManager をフルメッシュ対応に書き換え

**Files:**
- Modify: `crates/cplp-network/src/p2p.rs`
- Modify: `crates/cplp-network/src/lib.rs`

**Step 1: Write the failing tests**

`crates/cplp-network/src/p2p.rs` テストモジュールを更新:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_manager_has_local_peer_id() {
        let manager = P2pManager::new(5000, PeerId::new("test-peer"));
        assert_eq!(manager.local_peer_id(), &PeerId::new("test-peer"));
        assert_eq!(manager.state(), &P2pState::Idle);
        assert!(manager.peers().is_empty());
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let mut manager = P2pManager::new(5000, PeerId::new("test-peer"));
        assert_eq!(manager.state(), &P2pState::Idle);
        manager.start_server().await.unwrap();
        assert_eq!(manager.state(), &P2pState::ServerStarted);
    }

    #[tokio::test]
    async fn test_invalid_state_transition() {
        let mut manager = P2pManager::new(5000, PeerId::new("test-peer"));
        let result = manager.connect_to_peer(
            PeerId::new("remote"),
            "[::1]:5001".parse().unwrap(),
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_peer_connected_half_to_connected() {
        let mut manager = P2pManager::new(5000, PeerId::new("test-peer"));
        manager.start_server().await.unwrap();

        // 相手が先に接続 → HalfConnected
        let remote = PeerId::new("remote");
        manager.on_peer_connected(remote.clone(), "[::1]:5001".parse().unwrap()).await.unwrap();
        assert_eq!(manager.state(), &P2pState::HalfConnected);
        assert_eq!(manager.peers().len(), 0); // チャネルなしではまだ peers に入らない
    }

    #[tokio::test]
    async fn test_mixer_state_accessible() {
        let manager = P2pManager::new(5000, PeerId::new("test-peer"));
        let state = manager.mixer_state();
        assert!(state.tracks.is_empty());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p cplp-network -- p2p`
Expected: FAIL - new signature mismatch

**Step 3: Rewrite P2pManager**

`crates/cplp-network/src/p2p.rs` を書き換え。主な変更点:
- `PeerId` をコンストラクタ引数に追加
- `peers: HashMap<PeerId, PeerConnection>` 追加
- `mixer_state: MixerState` 追加
- `connect_to_peer()` に `PeerId` 引数追加
- `on_peer_connected()` に `PeerId` 引数追加
- Unison の `ProtocolServer` / `ProtocolClient` フィールド追加（TODO コメントのまま）

```rust
//! P2pManager: フルメッシュ P2P 接続管理
//!
//! REQ-NET-001: Unison Protocol による対等 P2P 接続
//! 各ピアが ProtocolServer + ProtocolClient のデュアルロールで動作
//! 最大5人のフルメッシュ接続をサポート

use std::collections::HashMap;
use std::net::SocketAddr;

use cplp_core::{CplpError, MixerState, PeerId, PeerStatus, TrackState};
use tokio::sync::{mpsc, watch};

use crate::audio_channel::AudioStreamer;

/// P2P 接続状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2pState {
    Idle,
    ServerStarted,
    Connecting,
    HalfConnected,
    Connected,
    SessionActive,
    Disconnecting,
}

/// P2P 接続イベント
#[derive(Debug)]
pub enum P2pEvent {
    StateChanged(P2pState),
    PeerConnected { peer_id: PeerId, addr: SocketAddr },
    PeerDisconnected { peer_id: PeerId },
    Error(CplpError),
}

/// ピア接続情報
pub struct PeerConnection {
    pub addr: SocketAddr,
    pub status: PeerStatus,
    // TODO: Unison チャネル統合後に追加
    // pub audio_channel: UnisonChannel,
    // pub control_channel: UnisonChannel,
}

/// P2pManager: フルメッシュ P2P 接続のオーケストレーター
pub struct P2pManager {
    state: P2pState,
    local_peer_id: PeerId,
    listen_addr: SocketAddr,
    peers: HashMap<PeerId, PeerConnection>,
    mixer_state: MixerState,
    state_tx: watch::Sender<P2pState>,
    state_rx: watch::Receiver<P2pState>,
    event_tx: mpsc::Sender<P2pEvent>,
    event_rx: Option<mpsc::Receiver<P2pEvent>>,
    // TODO: Unison API 統合
    // server: Option<ProtocolServer>,
    // server_handle: Option<ServerHandle>,
}

impl P2pManager {
    pub fn new(listen_port: u16, local_peer_id: PeerId) -> Self {
        let listen_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], listen_port));
        let (state_tx, state_rx) = watch::channel(P2pState::Idle);
        let (event_tx, event_rx) = mpsc::channel(64);

        Self {
            state: P2pState::Idle,
            local_peer_id,
            listen_addr,
            peers: HashMap::new(),
            mixer_state: MixerState::new(),
            state_tx,
            state_rx,
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    pub fn peers(&self) -> &HashMap<PeerId, PeerConnection> {
        &self.peers
    }

    pub fn mixer_state(&self) -> &MixerState {
        &self.mixer_state
    }

    pub fn mixer_state_mut(&mut self) -> &mut MixerState {
        &mut self.mixer_state
    }

    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<P2pEvent>> {
        self.event_rx.take()
    }

    pub fn state_rx(&self) -> watch::Receiver<P2pState> {
        self.state_rx.clone()
    }

    pub fn state(&self) -> &P2pState {
        &self.state
    }

    fn transition(&mut self, new_state: P2pState) {
        tracing::info!("P2P state: {:?} → {:?}", self.state, new_state);
        self.state = new_state.clone();
        let _ = self.state_tx.send(new_state.clone());
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(P2pEvent::StateChanged(new_state)).await;
        });
    }

    pub async fn start_server(&mut self) -> Result<(), CplpError> {
        if self.state != P2pState::Idle {
            return Err(CplpError::Network(format!(
                "サーバー起動には Idle 状態が必要（現在: {:?}）",
                self.state
            )));
        }

        // TODO: Unison ProtocolServer 起動
        tracing::info!("P2P server starting on {}", self.listen_addr);
        self.transition(P2pState::ServerStarted);
        Ok(())
    }

    pub async fn connect_to_peer(
        &mut self,
        peer_id: PeerId,
        peer_addr: SocketAddr,
    ) -> Result<(), CplpError> {
        if self.state != P2pState::ServerStarted && self.state != P2pState::SessionActive {
            return Err(CplpError::Network(format!(
                "接続には ServerStarted or SessionActive 状態が必要（現在: {:?}）",
                self.state
            )));
        }

        tracing::info!("Connecting to peer: {} at {}", peer_id, peer_addr);

        // TODO: Unison ProtocolClient で接続、チャネル開設
        // 接続確立後に peers に追加

        if self.state == P2pState::ServerStarted {
            self.transition(P2pState::HalfConnected);
        }

        Ok(())
    }

    pub async fn on_peer_connected(
        &mut self,
        peer_id: PeerId,
        peer_addr: SocketAddr,
    ) -> Result<(), CplpError> {
        tracing::info!("Peer connected: {} from {}", peer_id, peer_addr);

        let tx = self.event_tx.clone();
        let pid = peer_id.clone();
        tokio::spawn(async move {
            let _ = tx.send(P2pEvent::PeerConnected { peer_id: pid, addr: peer_addr }).await;
        });

        match self.state {
            P2pState::ServerStarted => {
                self.transition(P2pState::HalfConnected);
            }
            P2pState::HalfConnected => {
                self.transition(P2pState::Connected);
            }
            P2pState::SessionActive => {
                // 途中参加: セッション中に新ピアが接続
                tracing::info!("Late join: {} during active session", peer_id);
            }
            _ => {
                tracing::warn!("Unexpected peer connection in state: {:?}", self.state);
            }
        }

        Ok(())
    }

    /// ピアをメッシュに追加（チャネル確立後に呼ぶ）
    pub fn add_peer(&mut self, peer_id: PeerId, addr: SocketAddr, label: &str) {
        self.peers.insert(peer_id.clone(), PeerConnection {
            addr,
            status: PeerStatus::Connected,
        });
        self.mixer_state.add_track(peer_id, TrackState::new(label));
    }

    /// ピアをメッシュから削除
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.mixer_state.remove_track(peer_id);
    }

    pub async fn start_session(&mut self) -> Result<AudioStreamer, CplpError> {
        if self.state != P2pState::Connected && self.state != P2pState::SessionActive {
            return Err(CplpError::Network(format!(
                "セッション開始には Connected 状態が必要（現在: {:?}）",
                self.state
            )));
        }

        self.transition(P2pState::SessionActive);
        Ok(AudioStreamer::new())
    }

    pub async fn disconnect(&mut self) -> Result<(), CplpError> {
        tracing::info!("Disconnecting...");
        self.transition(P2pState::Disconnecting);

        // TODO: 全ピアに PeerLeft 送信、チャネル close、サーバー shutdown
        self.peers.clear();
        self.mixer_state = MixerState::new();

        self.transition(P2pState::Idle);
        Ok(())
    }
}
```

`crates/cplp-network/src/lib.rs` のエクスポートを更新:

```rust
pub mod audio_channel;
pub mod control;
pub mod p2p;

pub use audio_channel::AudioStreamer;
pub use control::{ControlEvent, ControlHandler};
pub use p2p::{P2pEvent, P2pManager, P2pState, PeerConnection};
```

**Step 4: Run tests**

Run: `cargo test -p cplp-network`
Expected: ALL PASS

**Step 5: Fix SessionManager compilation**

`crates/cplp-session/src/manager.rs` の `SessionManager::new()` を更新（`P2pManager::new()` のシグネチャが変わったため）:

```rust
// P2pManager::new(port) → P2pManager::new(port, PeerId)
let peer_id = PeerId::new(&format!("peer-{}", port));
Self {
    config,
    p2p: P2pManager::new(port, peer_id),
    // ...
}
```

`on_peer_connected` の呼び出しも更新:

```rust
Some(P2pEvent::PeerConnected { peer_id, addr }) => {
    self.p2p.on_peer_connected(peer_id, addr).await?;
    // ...
}
```

Run: `cargo test --workspace`
Expected: ALL PASS

**Step 6: Commit**

```bash
git add crates/cplp-network/src/p2p.rs crates/cplp-network/src/lib.rs crates/cplp-session/src/manager.rs
git commit -m "feat: P2pManager をフルメッシュ対応に拡張

- HashMap<PeerId, PeerConnection> でN-1本のピア接続管理
- MixerState をP2pManagerが直接保持
- add_peer/remove_peer でメッシュ動的更新
- SessionActive 中の途中参加をサポート
- PeerId をコンストラクタ引数に追加"
```

---

### Task 6: AudioStreamer を N ピア対応に拡張

**Files:**
- Modify: `crates/cplp-network/src/audio_channel.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn test_multi_peer_streamer() {
    let mut streamer = AudioStreamer::new();
    let peer_a = PeerId::new("peer-a");
    let peer_b = PeerId::new("peer-b");

    streamer.add_peer_track(peer_a.clone());
    streamer.add_peer_track(peer_b.clone());

    // 各ピアの受信キューにパケットをプッシュ
    let packet_a = AudioPacket { seq: 0, timestamp: 0, pcm_data: vec![0.5, 0.5] };
    let packet_b = AudioPacket { seq: 0, timestamp: 0, pcm_data: vec![0.3, 0.3] };

    streamer.push_received(&peer_a, packet_a).await.unwrap();
    streamer.push_received(&peer_b, packet_b).await.unwrap();

    // 各ピアから受信できる
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p cplp-network -- audio`
Expected: FAIL

**Step 3: Extend AudioStreamer**

`crates/cplp-network/src/audio_channel.rs` に追加:

- `peer_recv_txs: HashMap<PeerId, mpsc::Sender<AudioPacket>>` — 各ピアの受信キュー書き込み側
- `peer_recv_rxs: HashMap<PeerId, mpsc::Receiver<AudioPacket>>` — 各ピアの受信キュー読み取り側
- `add_peer_track()`, `remove_peer_track()`, `push_received(&PeerId, packet)`, `take_peer_recv_rx(&PeerId)`, `has_peer(&PeerId)`

既存の単一 recv_tx/rx は後方互換で維持（2人モードのショートカットとして）。

**Step 4: Run tests**

Run: `cargo test -p cplp-network`
Expected: ALL PASS

**Step 5: Commit**

```bash
git add crates/cplp-network/src/audio_channel.rs
git commit -m "feat: AudioStreamer を N ピア対応に拡張

- HashMap<PeerId, Receiver> で各ピアの受信トラックを個別管理
- add_peer_track/remove_peer_track で動的ピア追加・削除
- push_received(&PeerId, packet) でピア別に受信パケット投入"
```

---

### Task 7: Unison ProtocolServer/Client 統合（P2pManager）

**Files:**
- Modify: `crates/cplp-network/src/p2p.rs`

**Step 1: Import Unison types and add fields**

```rust
use unison::{
    ConnectionEvent, ProtocolClient, ProtocolServer, ServerHandle, UnisonChannel,
};
```

P2pManager に Unison フィールドを追加:

```rust
server_handle: Option<ServerHandle>,
/// Client → 各ピアの Server への接続
client_channels: HashMap<PeerId, PeerChannels>,
```

```rust
pub struct PeerChannels {
    pub audio: UnisonChannel,
    pub control: UnisonChannel,
}
```

**Step 2: Implement start_server() with Unison**

```rust
pub async fn start_server(&mut self) -> Result<(), CplpError> {
    let server = ProtocolServer::with_identity("cplp", "0.2.0", "club.chronista.cplp");

    // audio チャネルハンドラー登録
    server.register_channel("audio", |_ctx, _stream| async { Ok(()) }).await;
    server.register_channel("control", |_ctx, _stream| async { Ok(()) }).await;

    // 接続イベント購読
    let mut conn_rx = server.subscribe_connection_events().await;

    // サーバー起動
    let handle = server.spawn_listen(&self.listen_addr.to_string()).await
        .map_err(|e| CplpError::Network(format!("Unison server error: {}", e)))?;

    self.server_handle = Some(handle);
    self.transition(P2pState::ServerStarted);

    // 接続イベントを P2pEvent に変換する背景タスク
    let event_tx = self.event_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = conn_rx.recv().await {
            match event {
                ConnectionEvent::Connected { remote_addr, .. } => {
                    let _ = event_tx.send(P2pEvent::PeerConnected {
                        peer_id: PeerId::new(&remote_addr.to_string()),
                        addr: remote_addr,
                    }).await;
                }
                ConnectionEvent::Disconnected { remote_addr } => {
                    let _ = event_tx.send(P2pEvent::PeerDisconnected {
                        peer_id: PeerId::new(&remote_addr.to_string()),
                    }).await;
                }
            }
        }
    });

    Ok(())
}
```

**Step 3: Implement connect_to_peer() with Unison**

```rust
pub async fn connect_to_peer(
    &mut self,
    peer_id: PeerId,
    peer_addr: SocketAddr,
) -> Result<(), CplpError> {
    let mut client = ProtocolClient::new_default()
        .map_err(|e| CplpError::Network(format!("Client creation failed: {}", e)))?;

    client.connect(&peer_addr.to_string()).await
        .map_err(|e| CplpError::Network(format!("Connect failed: {}", e)))?;

    let audio_ch = client.open_channel("audio").await
        .map_err(|e| CplpError::Network(format!("Open audio channel failed: {}", e)))?;
    let control_ch = client.open_channel("control").await
        .map_err(|e| CplpError::Network(format!("Open control channel failed: {}", e)))?;

    self.client_channels.insert(peer_id.clone(), PeerChannels {
        audio: audio_ch,
        control: control_ch,
    });

    self.add_peer(peer_id, peer_addr, "Unknown");

    if self.state == P2pState::ServerStarted {
        self.transition(P2pState::HalfConnected);
    }

    Ok(())
}
```

**Step 4: Build check**

Run: `cargo check -p cplp-network`
Expected: compiles (or identify Unison API mismatches to fix)

**Step 5: Commit**

```bash
git add crates/cplp-network/src/p2p.rs
git commit -m "feat: P2pManager に Unison ProtocolServer/Client を統合

- start_server(): ProtocolServer.spawn_listen() + ConnectionEvent 購読
- connect_to_peer(): ProtocolClient.connect() + audio/control チャネル開設
- PeerChannels で UnisonChannel ペアを管理"
```

---

### Task 8: AudioStreamer send/recv ループに Unison を統合

**Files:**
- Modify: `crates/cplp-network/src/audio_channel.rs`

**Step 1: Implement run_send_loop with UnisonChannel**

```rust
pub async fn run_send_loop(
    mut send_rx: mpsc::Receiver<AudioPacket>,
    channels: Vec<UnisonChannel>,  // 全ピアの audio チャネル
) -> Result<(), CplpError> {
    while let Some(packet) = send_rx.recv().await {
        let bytes = packet.to_bytes();
        for ch in &channels {
            if let Err(e) = ch.send_raw(&bytes).await {
                tracing::warn!("Audio send failed: {}", e);
            }
        }
        tracing::trace!("Sent audio packet seq={} to {} peers", packet.seq, channels.len());
    }
    Ok(())
}
```

**Step 2: Implement run_recv_loop with UnisonChannel**

```rust
pub async fn run_recv_loop(
    peer_id: PeerId,
    recv_tx: mpsc::Sender<AudioPacket>,
    channel: UnisonChannel,
) -> Result<(), CplpError> {
    loop {
        match channel.recv_raw().await {
            Ok(bytes) => {
                match AudioPacket::from_bytes(&bytes) {
                    Ok(packet) => {
                        if recv_tx.send(packet).await.is_err() {
                            tracing::debug!("Recv queue closed for {}", peer_id);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Invalid audio packet from {}: {}", peer_id, e);
                    }
                }
            }
            Err(_) => {
                tracing::info!("Audio channel closed for {}", peer_id);
                break;
            }
        }
    }
    Ok(())
}
```

**Step 3: Build check**

Run: `cargo check -p cplp-network`

**Step 4: Commit**

```bash
git add crates/cplp-network/src/audio_channel.rs
git commit -m "feat: AudioStreamer send/recv ループに Unison raw bytes を統合

- run_send_loop: 全ピアの audio チャネルに send_raw
- run_recv_loop: ピアごとに recv_raw → AudioPacket パース → mpsc"
```

---

### Task 9: ControlHandler に Unison チャネル統合

**Files:**
- Modify: `crates/cplp-network/src/control.rs`

**Step 1: Add broadcast/receive methods**

```rust
use unison::UnisonChannel;

impl ControlHandler {
    /// 全ピアにイベントを broadcast
    pub async fn broadcast(
        &self,
        channels: &HashMap<PeerId, UnisonChannel>,
        event: &ControlEvent,
    ) -> Result<(), CplpError> {
        let json = serde_json::to_value(event)
            .map_err(|e| CplpError::Network(format!("Serialize error: {}", e)))?;
        for (peer_id, ch) in channels {
            if let Err(e) = ch.send_event("control", json.clone()).await {
                tracing::warn!("Control send failed to {}: {}", peer_id, e);
            }
        }
        Ok(())
    }

    /// 1ピアからの control イベントを受信して MixerState に適用
    pub async fn run_recv_loop(
        &mut self,
        peer_id: PeerId,
        channel: UnisonChannel,
    ) -> Result<(), CplpError> {
        loop {
            match channel.recv().await {
                Ok(msg) => {
                    match msg.payload_as_value() {
                        Ok(value) => {
                            if let Ok(event) = serde_json::from_value::<ControlEvent>(value) {
                                self.apply_event(&event);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Invalid control message from {}: {}", peer_id, e);
                        }
                    }
                }
                Err(_) => {
                    tracing::info!("Control channel closed for {}", peer_id);
                    break;
                }
            }
        }
        Ok(())
    }
}
```

**Step 2: Build check**

Run: `cargo check -p cplp-network`

**Step 3: Commit**

```bash
git add crates/cplp-network/src/control.rs
git commit -m "feat: ControlHandler に Unison チャネル broadcast/recv を統合

- broadcast(): 全ピアに ControlEvent を JSON で送信
- run_recv_loop(): ピアからの ControlEvent 受信 → MixerState 適用"
```

---

### Task 10: SessionManager をフルメッシュ対応に更新

**Files:**
- Modify: `crates/cplp-session/src/manager.rs`

**Step 1: Update SessionManager**

主な変更:
- `P2pManager::new()` に `PeerId` を渡す
- `wait_for_connection()` を N ピア対応に
- `P2pEvent::PeerConnected` のフィールド名変更に追従
- `join()` で複数ピアに接続するフローに更新

**Step 2: Update tests**

既存の4テストを新しいシグネチャに合わせて更新。

**Step 3: Run tests**

Run: `cargo test --workspace`
Expected: ALL PASS

**Step 4: Commit**

```bash
git add crates/cplp-session/src/manager.rs
git commit -m "feat: SessionManager をフルメッシュ対応に更新

- PeerId をセッション作成時に生成
- N ピア接続フローに対応
- PeerConnected イベント形式の更新に追従"
```

---

## Phase 2: ロビーサーバー（Axum + SurrealDB）

### Task 11: cplp-lobby クレートのスキャフォールド

**Files:**
- Create: `crates/cplp-lobby/Cargo.toml`
- Create: `crates/cplp-lobby/src/main.rs`
- Create: `crates/cplp-lobby/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Add to workspace**

`Cargo.toml` の members に `"crates/cplp-lobby"` を追加。workspace.dependencies に追加:

```toml
axum = "0.8"
axum-extra = { version = "0.10", features = ["typed-header"] }
tower-http = { version = "0.6", features = ["cors"] }
surrealdb = "2"
oauth2 = "5"
jsonwebtoken = "9"
uuid = { version = "1", features = ["v4"] }
```

**Step 2: Create minimal crate**

`crates/cplp-lobby/Cargo.toml`:
```toml
[package]
name = "cplp-lobby"
version.workspace = true
edition.workspace = true

[[bin]]
name = "cplp-lobby"
path = "src/main.rs"

[dependencies]
cplp-core.workspace = true
axum.workspace = true
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

`src/main.rs`: 最小の Axum サーバー（health check のみ）

**Step 3: Build check**

Run: `cargo check -p cplp-lobby`

**Step 4: Commit**

```bash
git add crates/cplp-lobby/ Cargo.toml
git commit -m "chore: cplp-lobby クレートをスキャフォールド (Axum)"
```

---

### Task 12-16: ロビーサーバー各機能

以下は Phase 2 の残りタスク（概要のみ、実装時に詳細化）:

- **Task 12**: SurrealDB 接続 + スキーマ初期化
- **Task 13**: OAuth フロー（GitHub → Google → Discord）
- **Task 14**: グループ CRUD API
- **Task 15**: セッション管理 API
- **Task 16**: WebSocket（プレゼンス + セッション通知）

---

## Phase 3: クライアント統合

### Task 17-19: cplp-app とロビーサーバーの統合

- **Task 17**: cplp-app にロビー接続クライアント追加
- **Task 18**: ロビー経由のピア発見 → フルメッシュ接続
- **Task 19**: 途中参加 / 再参加の E2E テスト

---

## テスト戦略

| レベル | 対象 | ツール |
|--------|------|--------|
| ユニット | MixerState, ControlEvent, AudioStreamer | `cargo test` |
| 統合 | P2pManager + Unison (ローカル2ピア) | `cargo test` + tokio::test |
| E2E | 3ピアフルメッシュ | 手動テスト（3プロセス起動） |
| ロビー | API エンドポイント | `cargo test` + axum::test |

---

**計画バージョン**: 0.2.0
**最終更新**: 2026-02-20
