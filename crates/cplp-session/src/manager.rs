//! SessionManager: P2P ジャムセッションのライフサイクル管理
//!
//! REQ-SESSION-001: セッション作成・参加のワークフロー
//!
//! P2pManager + ControlHandler を統合し、
//! セッションの全ライフサイクル（待機→接続→ストリーミング→切断）を管理する。

use std::net::SocketAddr;

use cplp_core::{AppConfig, CplpError, PeerId};
use cplp_network::{AudioStreamer, ControlHandler, P2pEvent, P2pManager, P2pState};
use tokio::sync::watch;

use crate::lobby::LobbyClient;
use crate::signaling::LobbyEvent;

/// セッション状態（外部監視用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// 初期化中
    Initializing,
    /// ピアの接続を待機中
    WaitingForPeer,
    /// P2P 双方向接続完了
    Connected,
    /// オーディオストリーミング中
    Streaming,
    /// 切断済み
    Disconnected,
}

/// SessionManager: P2P + Control の統合オーケストレーター
///
/// ```text
/// host():  Idle → ServerStarted → (wait) → Connected → SessionActive
/// join():  Idle → ServerStarted → Connecting → HalfConnected → Connected → SessionActive
/// ```
pub struct SessionManager {
    config: AppConfig,
    p2p: P2pManager,
    control: ControlHandler,
    state: SessionState,
    state_tx: watch::Sender<SessionState>,
    state_rx: watch::Receiver<SessionState>,
}

impl SessionManager {
    pub fn new(config: AppConfig) -> Self {
        let (state_tx, state_rx) = watch::channel(SessionState::Initializing);
        let port = config.network.listen_port;
        Self {
            config,
            p2p: P2pManager::new(port, PeerId::new(&format!("peer-{}", port))),
            control: ControlHandler::new(),
            state: SessionState::Initializing,
            state_tx,
            state_rx,
        }
    }

    /// ロビー user_id を PeerId として使用するコンストラクタ
    pub fn with_user_id(config: AppConfig, user_id: &str) -> Self {
        let (state_tx, state_rx) = watch::channel(SessionState::Initializing);
        let port = config.network.listen_port;
        Self {
            config,
            p2p: P2pManager::new(port, PeerId::new(user_id)),
            control: ControlHandler::new(),
            state: SessionState::Initializing,
            state_tx,
            state_rx,
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn state_rx(&self) -> watch::Receiver<SessionState> {
        self.state_rx.clone()
    }

    fn set_state(&mut self, new: SessionState) {
        tracing::info!("Session: {:?} → {:?}", self.state, new);
        self.state = new.clone();
        let _ = self.state_tx.send(new);
    }

    /// ホストとしてセッションを開始
    ///
    /// P2P サーバーを起動し、ピアの接続を待ち、AudioStreamer を返す。
    /// Unison 未統合時は `wait_for_connection()` で永久にブロックする（正常動作）。
    pub async fn host(&mut self) -> Result<AudioStreamer, CplpError> {
        self.p2p.start_server().await?;
        self.set_state(SessionState::WaitingForPeer);
        tracing::info!(
            "ピアの接続を待機中 (port {})",
            self.config.network.listen_port
        );

        self.wait_for_connection().await?;
        self.begin_streaming().await
    }

    /// ゲストとしてセッションに参加
    ///
    /// デュアルロール: P2P サーバー起動 + 相手に接続し、双方向接続を確立する。
    pub async fn join(&mut self, peer_addr: SocketAddr) -> Result<AudioStreamer, CplpError> {
        self.p2p.start_server().await?;
        // TODO: ピアIDの交換はハンドシェイク時に行う
        let remote_peer_id = PeerId::new(&format!("peer-{}", peer_addr.port()));
        self.p2p.connect_to_peer(remote_peer_id, peer_addr).await?;
        tracing::info!("双方向接続の完了を待機中...");

        self.wait_for_connection().await?;
        self.begin_streaming().await
    }

    /// 双方向接続の完了を待つ
    ///
    /// P2pEvent::PeerConnected を受信し、P2pState::Connected になるまでループ。
    /// Unison ServerHandle の ConnectionEvent がこのイベントを発火する。
    async fn wait_for_connection(&mut self) -> Result<(), CplpError> {
        let mut event_rx = self
            .p2p
            .take_event_rx()
            .ok_or_else(|| CplpError::Session("P2P イベントチャネルは既に取得済みです".into()))?;

        loop {
            match event_rx.recv().await {
                Some(P2pEvent::PeerConnected { peer_id, addr }) => {
                    self.p2p.on_peer_connected(peer_id, addr).await?;
                    if *self.p2p.state() == P2pState::Connected {
                        self.set_state(SessionState::Connected);
                        return Ok(());
                    }
                }
                Some(P2pEvent::Error(e)) => return Err(e),
                Some(_) => {}
                None => {
                    return Err(CplpError::Session(
                        "P2P イベントチャネルが閉じました".into(),
                    ));
                }
            }
        }
    }

    /// ストリーミング開始（Connected → SessionActive）
    async fn begin_streaming(&mut self) -> Result<AudioStreamer, CplpError> {
        let streamer = self.p2p.start_session().await?;
        self.set_state(SessionState::Streaming);
        Ok(streamer)
    }

    /// ロビー経由でホストとしてセッションを開始
    ///
    /// 1. HTTP でセッション作成
    /// 2. WebSocket でグループを購読
    /// 3. P2P サーバー起動
    /// 4. PeerJoined イベントで各ピアに P2P 接続
    #[tracing::instrument(skip_all, fields(group = %group_id))]
    pub async fn host_via_lobby(
        &mut self,
        lobby: &mut LobbyClient,
        group_id: &str,
    ) -> Result<AudioStreamer, CplpError> {
        // セッション作成
        let session = lobby
            .create_session(group_id)
            .await
            .map_err(|e| CplpError::Session(format!("セッション作成失敗: {e}")))?;
        tracing::info!("セッション作成: {}", session.id);

        // グループを購読
        lobby
            .subscribe_group(group_id)
            .map_err(|e| CplpError::Session(format!("グループ購読失敗: {e}")))?;

        // P2P サーバー起動
        self.p2p.start_server().await?;
        self.set_state(SessionState::WaitingForPeer);
        tracing::info!("ロビー経由でピアを待機中 (session: {})", session.id);

        // WebSocket から PeerJoined を受信したら P2P 接続（5分タイムアウト）
        let mut event_rx = lobby
            .take_event_rx()
            .ok_or_else(|| CplpError::Session("ロビーイベントチャネルは既に取得済みです".into()))?;

        let timeout_duration = std::time::Duration::from_secs(300);
        let peer_joined = async {
            while let Some(event) = event_rx.recv().await {
                match event {
                    LobbyEvent::PeerJoined { user_id, addr, .. } => {
                        tracing::info!("ピア参加検知: {} @ {}", user_id, addr);
                        let peer_addr: SocketAddr = addr
                            .parse()
                            .map_err(|e| CplpError::Session(format!("アドレスパース失敗: {e}")))?;
                        let peer_id = PeerId::new(&user_id);
                        self.p2p.connect_to_peer(peer_id, peer_addr).await?;
                        return Ok(());
                    }
                    _ => {
                        tracing::debug!("ロビーイベント (無視): {:?}", event);
                    }
                }
            }
            Err(CplpError::Session("ロビー接続が切断されました".into()))
        };

        tokio::time::timeout(timeout_duration, peer_joined)
            .await
            .map_err(|_| CplpError::Session("ピア待機がタイムアウトしました (5分)".into()))??;

        self.wait_for_connection().await?;
        self.begin_streaming().await
    }

    /// ロビー経由でゲストとしてセッションに参加
    ///
    /// 1. HTTP でセッション参加（ピアリスト取得）
    /// 2. P2P サーバー起動
    /// 3. 既存ピア全員に P2P 接続
    #[tracing::instrument(skip_all, fields(session = %session_id))]
    pub async fn join_via_lobby(
        &mut self,
        lobby: &mut LobbyClient,
        session_id: &str,
    ) -> Result<AudioStreamer, CplpError> {
        // セッション参加
        let join_resp = lobby
            .join_session(session_id)
            .await
            .map_err(|e| CplpError::Session(format!("セッション参加失敗: {e}")))?;
        tracing::info!(
            "セッション参加: {} (status: {}, peers: {})",
            session_id,
            join_resp.status,
            join_resp.peers.len()
        );

        // P2P サーバー起動
        self.p2p.start_server().await?;

        // 自分以外のピアに P2P 接続
        let my_addr = lobby.config().local_addr;
        for peer in &join_resp.peers {
            let peer_addr: SocketAddr = match peer.to_socket_addr() {
                Ok(addr) => addr,
                Err(e) => {
                    tracing::warn!("ピアアドレスのパース失敗 ({}): {}", peer.addr, e);
                    continue;
                }
            };
            // 自分自身には接続しない
            if peer_addr == my_addr {
                continue;
            }
            tracing::info!("ピアに接続: {} @ {}", peer.user_id, peer_addr);
            self.p2p
                .connect_to_peer(peer.to_peer_id(), peer_addr)
                .await?;
        }

        self.wait_for_connection().await?;
        self.begin_streaming().await
    }

    /// セッションを終了
    pub async fn shutdown(&mut self) -> Result<(), CplpError> {
        tracing::info!("セッションを終了中...");
        self.set_state(SessionState::Disconnected);
        self.p2p.disconnect().await
    }

    pub fn p2p(&self) -> &P2pManager {
        &self.p2p
    }

    pub fn p2p_mut(&mut self) -> &mut P2pManager {
        &mut self.p2p
    }

    pub fn control(&self) -> &ControlHandler {
        &self.control
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_session() {
        let session = SessionManager::new(AppConfig::default());
        assert_eq!(session.state(), &SessionState::Initializing);
    }

    #[tokio::test]
    async fn test_with_user_id() {
        let session = SessionManager::with_user_id(AppConfig::default(), "users:player1");
        assert_eq!(session.state(), &SessionState::Initializing);
        // PeerId がロビーの user_id で初期化されていることを確認
        assert_eq!(session.p2p().local_peer_id(), &PeerId::new("users:player1"));
    }

    #[tokio::test]
    async fn test_host_transitions_to_waiting() {
        let mut session = SessionManager::new(AppConfig::default());
        // host() は wait_for_connection() でブロックするので、
        // 部分的に状態遷移をテスト
        session.p2p.start_server().await.unwrap();
        session.set_state(SessionState::WaitingForPeer);
        assert_eq!(session.state(), &SessionState::WaitingForPeer);
        assert_eq!(session.p2p.state(), &P2pState::ServerStarted);
    }

    #[tokio::test]
    async fn test_state_watch() {
        let mut session = SessionManager::new(AppConfig::default());
        let mut rx = session.state_rx();

        session.set_state(SessionState::WaitingForPeer);
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), SessionState::WaitingForPeer);

        session.set_state(SessionState::Connected);
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), SessionState::Connected);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let mut session = SessionManager::new(AppConfig::default());
        session.shutdown().await.unwrap();
        assert_eq!(session.state(), &SessionState::Disconnected);
    }
}
