use cplp_core::PeerStatus;

/// P2P 接続を管理する中心的な構造体
///
/// REQ-NET-001: Unison Protocol による対等 P2P 接続
/// 各ピアが ProtocolServer + ProtocolClient のデュアルロールで動作
pub struct P2pManager {
    pub state: P2pState,
    pub peer_status: Option<PeerStatus>,
    // TODO: ProtocolServer, ProtocolClient (unison-protocol)
}

/// P2P 接続状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2pState {
    Idle,
    Listening,
    Connecting,
    HalfConnected,
    Connected,
    SessionActive,
    Disconnecting,
}
