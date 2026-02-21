//! ロビーサーバーとの通信型定義
//!
//! REQ-NET-002: ロビー経由のピアアドレス交換
//!
//! サーバー側 `cplp-lobby::ws` の WsEvent / WsCommand と同一 JSON フォーマット。
//! クライアント側でデシリアライズに使用する。

use std::net::SocketAddr;

use cplp_core::PeerId;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 設定
// ---------------------------------------------------------------------------

/// ロビーサーバーへの接続設定
#[derive(Debug, Clone)]
pub struct LobbyConfig {
    /// ロビーサーバーの HTTP ベース URL (例: "http://localhost:3000")
    pub base_url: String,
    /// JWT 認証トークン
    pub token: String,
    /// ローカルの P2P リスンアドレス（他ピアに公開する）
    pub local_addr: SocketAddr,
}

// ---------------------------------------------------------------------------
// WebSocket イベント (Server → Client)
// ---------------------------------------------------------------------------

/// ロビーサーバーから受信するリアルタイムイベント
///
/// cplp-lobby の WsEvent と同一構造（`#[serde(tag = "type")]`）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LobbyEvent {
    SessionStarted {
        group_id: String,
        session_id: String,
        started_by: String,
    },
    PeerJoined {
        session_id: String,
        user_id: String,
        addr: String,
    },
    PeerLeft {
        session_id: String,
        user_id: String,
    },
    Presence {
        user_id: String,
        status: String,
    },
}

// ---------------------------------------------------------------------------
// WebSocket コマンド (Client → Server)
// ---------------------------------------------------------------------------

/// ロビーサーバーへ送信するコマンド
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LobbyCommand {
    SubscribeGroup { group_id: String },
    SetPresence { status: String },
}

// ---------------------------------------------------------------------------
// ピア情報
// ---------------------------------------------------------------------------

/// ロビーから取得したピア情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPeerInfo {
    pub user_id: String,
    pub addr: String,
}

impl LobbyPeerInfo {
    /// user_id から PeerId に変換
    pub fn to_peer_id(&self) -> PeerId {
        PeerId::new(&self.user_id)
    }

    /// addr 文字列を SocketAddr にパース
    pub fn to_socket_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        self.addr.parse()
    }
}

// ---------------------------------------------------------------------------
// HTTP API レスポンス型
// ---------------------------------------------------------------------------

/// GET /groups レスポンスの1要素
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
}

/// POST /groups/{group_id}/sessions レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub id: String,
    pub status: String,
}

/// POST /sessions/{id}/join レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSessionResponse {
    pub status: String,
    pub peers: Vec<LobbyPeerInfo>,
}

/// GET /sessions/{id}/peers レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeersResponse {
    pub peers: Vec<LobbyPeerInfo>,
}

/// POST /sessions/{id}/leave レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveSessionResponse {
    pub status: String,
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lobby_event_peer_joined_roundtrip() {
        let event = LobbyEvent::PeerJoined {
            session_id: "sessions:abc".into(),
            user_id: "users:u1".into(),
            addr: "[::1]:5000".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"PeerJoined\""));

        let parsed: LobbyEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            LobbyEvent::PeerJoined { user_id, addr, .. } => {
                assert_eq!(user_id, "users:u1");
                assert_eq!(addr, "[::1]:5000");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_lobby_command_subscribe_roundtrip() {
        let cmd = LobbyCommand::SubscribeGroup {
            group_id: "groups:testband".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: LobbyCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            LobbyCommand::SubscribeGroup { group_id } => {
                assert_eq!(group_id, "groups:testband");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_lobby_peer_info_to_peer_id() {
        let info = LobbyPeerInfo {
            user_id: "users:player1".into(),
            addr: "[::1]:5000".into(),
        };
        assert_eq!(info.to_peer_id(), PeerId::new("users:player1"));
    }

    #[test]
    fn test_lobby_peer_info_to_socket_addr() {
        let info = LobbyPeerInfo {
            user_id: "users:player1".into(),
            addr: "[::1]:5000".into(),
        };
        let addr = info.to_socket_addr().unwrap();
        assert_eq!(addr.port(), 5000);
    }

    #[test]
    fn test_create_session_response_deserialize() {
        let json = r#"{"id":"sessions:abc123","status":"waiting"}"#;
        let resp: CreateSessionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "sessions:abc123");
        assert_eq!(resp.status, "waiting");
    }

    #[test]
    fn test_join_session_response_with_peers() {
        let json = r#"{
            "status": "active",
            "peers": [
                {"user_id": "users:host", "addr": "[::1]:5000"},
                {"user_id": "users:guest", "addr": "[::1]:5001"}
            ]
        }"#;
        let resp: JoinSessionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "active");
        assert_eq!(resp.peers.len(), 2);
        assert_eq!(resp.peers[0].user_id, "users:host");
    }
}
