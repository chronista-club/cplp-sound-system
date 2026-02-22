//! ControlHandler: ミキサー制御 + セッション管理
//!
//! REQ-NET-003: QUIC 上の独立チャネルによるオーディオ/コントロール分離
//! REQ-MIXER-001: 共有ミキサー状態の同期

use std::collections::HashMap;
use std::net::SocketAddr;

use cplp_core::{CplpError, MixerState, PeerId, TrackState};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use unison::UnisonChannel;

/// control チャネルイベント（全ピア間で送受信）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ControlEvent {
    // ── ミキサー操作 ──
    FaderChange {
        track: PeerId,
        volume: f32,
        ts: u64,
    },
    PanChange {
        track: PeerId,
        pan: f32,
        ts: u64,
    },
    MuteToggle {
        track: PeerId,
        mute: bool,
        ts: u64,
    },
    SoloToggle {
        track: PeerId,
        solo: bool,
        ts: u64,
    },
    MasterVol {
        volume: f32,
        ts: u64,
    },

    // ── セッション管理 ──
    PeerJoined {
        peer: PeerId,
        addr: SocketAddr,
        label: String,
    },
    PeerLeft {
        peer: PeerId,
    },
    /// 途中参加者へのミキサー全状態同期
    MixerSync {
        state: MixerState,
    },

    // ── モニタリング ──
    LatencyReport {
        rtt_us: u64,
        jitter_us: u64,
    },

    // ── プラグイン情報 ──
    PluginInfo {
        name: String,
        vendor: String,
    },
    PluginChanged {
        name: String,
        vendor: String,
    },

    // ── Cadence コマンド ──
    Command {
        from: PeerId,
        mode: CommandMode,
        text: String,
    },
    CommandAck {
        status: CommandStatus,
        message: String,
    },
    PluginSwitch {
        from: PeerId,
        plugin_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandMode {
    Parse,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandStatus {
    Accepted,
    Rejected,
    Error,
}

/// ControlHandler: control チャネルの処理
///
/// 各ピアとの control チャネルを管理し、
/// ミキサーイベントの送受信と MixerState の更新を行う。
pub struct ControlHandler {
    mixer_state: MixerState,
}

impl Default for ControlHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlHandler {
    pub fn new() -> Self {
        Self {
            mixer_state: MixerState::new(),
        }
    }

    pub fn mixer_state(&self) -> &MixerState {
        &self.mixer_state
    }

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
                self.mixer_state
                    .add_track(peer.clone(), TrackState::new(label));
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

    /// 全ピアに ControlEvent を broadcast
    pub async fn broadcast(
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
}

/// 1ピアからの control イベントを受信するループ
///
/// 各ピアにつき1つの受信ループが起動される。
/// 受信したイベントは event_tx に送信し、呼び出し側が apply_event() する。
pub async fn run_control_recv_loop(
    peer_id: PeerId,
    channel: UnisonChannel,
    event_tx: mpsc::Sender<(PeerId, ControlEvent)>,
) -> Result<(), CplpError> {
    loop {
        match channel.recv().await {
            Ok(msg) => match msg.payload_as_value() {
                Ok(value) => match serde_json::from_value::<ControlEvent>(value) {
                    Ok(event) => {
                        if event_tx.send((peer_id.clone(), event)).await.is_err() {
                            tracing::debug!("Control event queue closed for {}", peer_id);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Invalid control event from {}: {}", peer_id, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Invalid control message from {}: {}", peer_id, e);
                }
            },
            Err(e) => {
                tracing::info!("Control channel closed for {}: {}", peer_id, e);
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cplp_core::{MixerState, PeerId, TrackState};

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

    #[test]
    fn test_apply_event_fader() {
        let mut handler = ControlHandler::new();
        let peer = PeerId::new("p1");
        handler
            .mixer_state_mut()
            .add_track(peer.clone(), TrackState::new("Bass"));

        let event = ControlEvent::FaderChange {
            track: peer.clone(),
            volume: 0.7,
            ts: 100,
        };
        handler.apply_event(&event);
        assert_eq!(handler.mixer_state().tracks[&peer].volume, 0.7);
    }

    #[test]
    fn test_apply_event_peer_joined() {
        let mut handler = ControlHandler::new();
        let event = ControlEvent::PeerJoined {
            peer: PeerId::new("p1"),
            addr: "[::1]:5000".parse().unwrap(),
            label: "Guitar".to_string(),
        };
        handler.apply_event(&event);
        assert_eq!(handler.mixer_state().tracks.len(), 1);
    }

    #[test]
    fn test_apply_event_peer_left() {
        let mut handler = ControlHandler::new();
        let peer = PeerId::new("p1");
        handler
            .mixer_state_mut()
            .add_track(peer.clone(), TrackState::new("Synth"));

        let event = ControlEvent::PeerLeft { peer: peer.clone() };
        handler.apply_event(&event);
        assert!(handler.mixer_state().tracks.is_empty());
    }

    #[test]
    fn command_event_serialization() {
        let event = ControlEvent::Command {
            from: PeerId::new("player-a"),
            mode: CommandMode::Parse,
            text: "C major scale 120bpm".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ControlEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ControlEvent::Command { from, mode, text } => {
                assert_eq!(from, PeerId::new("player-a"));
                assert!(matches!(mode, CommandMode::Parse));
                assert_eq!(text, "C major scale 120bpm");
            }
            _ => panic!("Expected Command variant"),
        }
    }

    #[test]
    fn command_ack_serialization() {
        let event = ControlEvent::CommandAck {
            status: CommandStatus::Accepted,
            message: "演奏開始".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ControlEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ControlEvent::CommandAck { status, message } => {
                assert!(matches!(status, CommandStatus::Accepted));
                assert_eq!(message, "演奏開始");
            }
            _ => panic!("Expected CommandAck variant"),
        }
    }

    #[test]
    fn plugin_switch_serialization() {
        let event = ControlEvent::PluginSwitch {
            from: PeerId::new("player-a"),
            plugin_id: "com.u-he.Diva".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("PluginSwitch"));
        let parsed: ControlEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ControlEvent::PluginSwitch { from, plugin_id } => {
                assert_eq!(from, PeerId::new("player-a"));
                assert_eq!(plugin_id, "com.u-he.Diva");
            }
            _ => panic!("Expected PluginSwitch variant"),
        }
    }
}
