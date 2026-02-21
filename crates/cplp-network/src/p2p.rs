//! P2pManager: デュアルロール P2P 接続管理（フルメッシュ対応）
//!
//! REQ-NET-001: Unison Protocol による対等 P2P 接続
//! 各ピアが ProtocolServer + ProtocolClient のデュアルロールで動作
//! N ピア対応: HashMap<PeerId, PeerConnection> でフルメッシュ管理

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use cplp_core::{CplpError, MixerState, PeerId, PeerStatus, TrackState};
use tokio::sync::{mpsc, watch};
use unison::network::UnisonStream;
use unison::network::context::ConnectionContext;
use unison::{ConnectionEvent, ProtocolClient, ProtocolServer, ServerHandle, UnisonChannel};

use crate::audio_channel::AudioStreamer;

/// P2P 接続状態
///
/// spec/03 §4.2 の状態遷移図に対応:
/// Idle → ServerStarted → Connecting → HalfConnected → Connected → SessionActive
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2pState {
    /// 初期状態
    Idle,
    /// ProtocolServer が listen 中
    ServerStarted,
    /// 相手への接続試行中
    Connecting,
    /// 片方向の接続が確立
    HalfConnected,
    /// 双方向の接続が確立
    Connected,
    /// チャネル開設完了、セッション中
    SessionActive,
    /// 切断処理中
    Disconnecting,
}

/// ピアとの通信チャネルペア
pub struct PeerChannels {
    pub audio: UnisonChannel,
    pub control: UnisonChannel,
}

/// ピア接続情報
pub struct PeerConnection {
    pub addr: SocketAddr,
    pub status: PeerStatus,
    pub channels: Option<PeerChannels>,
}

impl fmt::Debug for PeerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerConnection")
            .field("addr", &self.addr)
            .field("status", &self.status)
            .field("channels", &self.channels.as_ref().map(|_| "..."))
            .finish()
    }
}

/// P2P 接続イベント
#[derive(Debug)]
pub enum P2pEvent {
    /// 状態が変化した
    StateChanged(P2pState),
    /// ピアが接続してきた
    PeerConnected { peer_id: PeerId, addr: SocketAddr },
    /// ピアが切断した
    PeerDisconnected { peer_id: PeerId },
    /// エラーが発生した
    Error(CplpError),
}

/// P2pManager: デュアルロール P2P 接続のオーケストレーター
///
/// 各ピアが ProtocolServer + ProtocolClient を同時に持ち、
/// 相手のアドレスを知ったら双方向に接続する。
/// N ピア対応: peers HashMap でフルメッシュ管理。
pub struct P2pManager {
    state: P2pState,
    local_peer_id: PeerId,
    peers: HashMap<PeerId, PeerConnection>,
    mixer_state: MixerState,
    listen_addr: SocketAddr,
    /// 状態変更の通知
    state_tx: watch::Sender<P2pState>,
    state_rx: watch::Receiver<P2pState>,
    /// イベント通知
    event_tx: mpsc::Sender<P2pEvent>,
    event_rx: Option<mpsc::Receiver<P2pEvent>>,
    /// Unison サーバーハンドル
    server_handle: Option<ServerHandle>,
}

impl P2pManager {
    /// 新しい P2pManager を作成
    pub fn new(listen_port: u16, local_peer_id: PeerId) -> Self {
        let listen_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], listen_port));
        let (state_tx, state_rx) = watch::channel(P2pState::Idle);
        let (event_tx, event_rx) = mpsc::channel(64);

        Self {
            state: P2pState::Idle,
            local_peer_id,
            peers: HashMap::new(),
            mixer_state: MixerState::new(),
            listen_addr,
            state_tx,
            state_rx,
            event_tx,
            event_rx: Some(event_rx),
            server_handle: None,
        }
    }

    /// ローカル PeerId を取得
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// 接続中のピア一覧を取得
    pub fn peers(&self) -> &HashMap<PeerId, PeerConnection> {
        &self.peers
    }

    /// 共有ミキサー状態を取得
    pub fn mixer_state(&self) -> &MixerState {
        &self.mixer_state
    }

    /// 共有ミキサー状態を可変で取得
    pub fn mixer_state_mut(&mut self) -> &mut MixerState {
        &mut self.mixer_state
    }

    /// イベント受信チャネルを取得（一度だけ呼べる）
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<P2pEvent>> {
        self.event_rx.take()
    }

    /// 状態の監視チャネルを取得
    pub fn state_rx(&self) -> watch::Receiver<P2pState> {
        self.state_rx.clone()
    }

    /// 現在の状態を取得
    pub fn state(&self) -> &P2pState {
        &self.state
    }

    /// ピアを追加（接続情報 + ミキサートラック）
    pub fn add_peer(&mut self, peer_id: PeerId, addr: SocketAddr, label: &str) {
        self.peers.insert(
            peer_id.clone(),
            PeerConnection {
                addr,
                status: PeerStatus::Connected,
                channels: None,
            },
        );
        self.mixer_state.add_track(peer_id, TrackState::new(label));
    }

    /// ピアを削除（接続情報 + ミキサートラック）
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.mixer_state.remove_track(peer_id);
    }

    /// 状態を遷移させる
    fn transition(&mut self, new_state: P2pState) {
        tracing::info!("P2P state: {:?} → {:?}", self.state, new_state);
        self.state = new_state.clone();
        let _ = self.state_tx.send(new_state.clone());
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(P2pEvent::StateChanged(new_state)).await;
        });
    }

    /// サーバーを起動して listen 開始
    ///
    /// Unison の spawn_listen() を使って非ブロッキングで起動する。
    pub async fn start_server(&mut self) -> Result<(), CplpError> {
        if self.state != P2pState::Idle {
            return Err(CplpError::Network(format!(
                "サーバー起動には Idle 状態が必要（現在: {:?}）",
                self.state
            )));
        }

        let server = ProtocolServer::with_identity("cplp", "0.2.0", "club.chronista.cplp");

        // チャネルハンドラー登録（Phase 2 で本格的にストリーム受け渡しを実装）
        server
            .register_channel(
                "audio",
                |_ctx: Arc<ConnectionContext>, stream: UnisonStream| async move {
                    let _channel = UnisonChannel::new(stream);
                    tracing::info!("Incoming audio channel accepted");
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    Ok(())
                },
            )
            .await;

        server
            .register_channel(
                "control",
                |_ctx: Arc<ConnectionContext>, stream: UnisonStream| async move {
                    let _channel = UnisonChannel::new(stream);
                    tracing::info!("Incoming control channel accepted");
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    Ok(())
                },
            )
            .await;

        // 接続イベントを購読（spawn_listen 前に取得）
        let mut conn_rx = server.subscribe_connection_events().await;

        // サーバー起動（spawn_listen は self を consume する）
        let listen_str = format!("[::]:{}", self.listen_addr.port());
        let handle = server
            .spawn_listen(&listen_str)
            .await
            .map_err(|e| CplpError::Network(format!("Unison server start failed: {}", e)))?;

        tracing::info!("P2P server listening on {}", handle.local_addr());
        self.server_handle = Some(handle);
        self.transition(P2pState::ServerStarted);

        // バックグラウンドタスク: 接続イベントを P2pEvent に転送
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = conn_rx.recv().await {
                match event {
                    ConnectionEvent::Connected { remote_addr, .. } => {
                        let _ = event_tx
                            .send(P2pEvent::PeerConnected {
                                peer_id: PeerId::new(&remote_addr.to_string()),
                                addr: remote_addr,
                            })
                            .await;
                    }
                    ConnectionEvent::Disconnected { remote_addr } => {
                        let _ = event_tx
                            .send(P2pEvent::PeerDisconnected {
                                peer_id: PeerId::new(&remote_addr.to_string()),
                            })
                            .await;
                    }
                }
            }
        });

        Ok(())
    }

    /// 相手のピアに接続
    ///
    /// spec/03 §4.1: 接続確立シーケンス
    /// ServerStarted または SessionActive（レイトジョイン）で接続可能
    pub async fn connect_to_peer(
        &mut self,
        peer_id: PeerId,
        peer_addr: SocketAddr,
    ) -> Result<(), CplpError> {
        if self.state != P2pState::ServerStarted && self.state != P2pState::SessionActive {
            return Err(CplpError::Network(format!(
                "接続には ServerStarted または SessionActive 状態が必要（現在: {:?}）",
                self.state
            )));
        }

        tracing::info!("Connecting to peer: {} at {}", peer_id, peer_addr);

        let mut client = ProtocolClient::new_default()
            .map_err(|e| CplpError::Network(format!("Client creation failed: {}", e)))?;

        let addr_str = format!("[::1]:{}", peer_addr.port());
        client
            .connect(&addr_str)
            .await
            .map_err(|e| CplpError::Network(format!("Connect to {} failed: {}", peer_addr, e)))?;

        let audio_ch = client
            .open_channel("audio")
            .await
            .map_err(|e| CplpError::Network(format!("Open audio channel failed: {}", e)))?;
        let control_ch = client
            .open_channel("control")
            .await
            .map_err(|e| CplpError::Network(format!("Open control channel failed: {}", e)))?;

        self.peers.insert(
            peer_id.clone(),
            PeerConnection {
                addr: peer_addr,
                status: PeerStatus::Connected,
                channels: Some(PeerChannels {
                    audio: audio_ch,
                    control: control_ch,
                }),
            },
        );
        self.mixer_state.add_track(peer_id, TrackState::new("Peer"));

        if self.state == P2pState::ServerStarted {
            self.transition(P2pState::HalfConnected);
        }

        Ok(())
    }

    /// 相手からの接続を受け入れた時のコールバック
    ///
    /// ServerHandle の ConnectionEvent で呼ばれる
    pub async fn on_peer_connected(
        &mut self,
        peer_id: PeerId,
        peer_addr: SocketAddr,
    ) -> Result<(), CplpError> {
        tracing::info!("Peer connected: {} from {}", peer_id, peer_addr);

        let tx = self.event_tx.clone();
        let pid = peer_id.clone();
        let addr = peer_addr;
        tokio::spawn(async move {
            let _ = tx
                .send(P2pEvent::PeerConnected { peer_id: pid, addr })
                .await;
        });

        match self.state {
            P2pState::ServerStarted => {
                // 相手が先に接続してきた（自分はまだ connect していない）
                self.transition(P2pState::HalfConnected);
            }
            P2pState::HalfConnected => {
                // 双方向接続完了
                self.add_peer(peer_id, peer_addr, "Peer");
                self.transition(P2pState::Connected);
            }
            P2pState::SessionActive => {
                // レイトジョイン: セッション中に新しいピアが参加
                self.add_peer(peer_id, peer_addr, "Peer");
                tracing::info!("Late join: peer added to active session");
            }
            _ => {
                tracing::warn!("Unexpected peer connection in state: {:?}", self.state);
            }
        }

        Ok(())
    }

    /// セッションを開始（チャネル開設完了後）
    ///
    /// Connected または SessionActive（レイトジョイン）で開始可能
    pub async fn start_session(&mut self) -> Result<AudioStreamer, CplpError> {
        if self.state != P2pState::Connected && self.state != P2pState::SessionActive {
            return Err(CplpError::Network(format!(
                "セッション開始には Connected または SessionActive 状態が必要（現在: {:?}）",
                self.state
            )));
        }

        // TODO: Unison チャネルから AudioStreamer を構築
        if self.state == P2pState::Connected {
            self.transition(P2pState::SessionActive);
        }

        Ok(AudioStreamer::new())
    }

    /// 切断
    pub async fn disconnect(&mut self) -> Result<(), CplpError> {
        tracing::info!("Disconnecting...");
        self.transition(P2pState::Disconnecting);

        self.peers.clear();
        self.mixer_state = MixerState::new();

        if let Some(handle) = self.server_handle.take() {
            let _ = handle.shutdown().await;
        }

        self.transition(P2pState::Idle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cplp_core::PeerId;

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
        let result = manager
            .connect_to_peer(PeerId::new("remote"), "[::1]:5001".parse().unwrap())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_add_remove_peer() {
        let mut manager = P2pManager::new(5000, PeerId::new("test-peer"));
        let remote = PeerId::new("remote");
        manager.add_peer(remote.clone(), "[::1]:5001".parse().unwrap(), "Guitar");
        assert_eq!(manager.peers().len(), 1);
        assert!(manager.mixer_state().tracks.contains_key(&remote));

        manager.remove_peer(&remote);
        assert!(manager.peers().is_empty());
        assert!(manager.mixer_state().tracks.is_empty());
    }

    #[tokio::test]
    async fn test_mixer_state_accessible() {
        let manager = P2pManager::new(5000, PeerId::new("test-peer"));
        assert!(manager.mixer_state().tracks.is_empty());
    }
}
