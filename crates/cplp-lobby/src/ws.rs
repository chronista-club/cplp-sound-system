//! WebSocket プレゼンス + セッション通知
//!
//! REQ-LOBBY-005: リアルタイム通知

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};

// ---------------------------------------------------------------------------
// 型定義
// ---------------------------------------------------------------------------

/// WebSocket イベント (Server → Client)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsEvent {
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

/// WebSocket コマンド (Client → Server)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsCommand {
    SubscribeGroup { group_id: String },
    SetPresence { status: String },
}

// ---------------------------------------------------------------------------
// 接続管理
// ---------------------------------------------------------------------------

/// グループ別 broadcast チャネルを管理する
#[derive(Clone, Default)]
pub struct ConnectionManager {
    /// group_id → broadcast sender
    groups: Arc<RwLock<HashMap<String, broadcast::Sender<WsEvent>>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// グループの broadcast チャネルを取得（なければ作成）
    pub async fn get_or_create_group(&self, group_id: &str) -> broadcast::Sender<WsEvent> {
        let mut groups = self.groups.write().await;
        groups
            .entry(group_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0)
            .clone()
    }

    /// グループにイベントを送信
    pub async fn broadcast_to_group(&self, group_id: &str, event: WsEvent) {
        let groups = self.groups.read().await;
        if let Some(tx) = groups.get(group_id) {
            let _ = tx.send(event);
        }
    }
}

/// グローバル接続マネージャ
pub(crate) static CONNECTIONS: LazyLock<ConnectionManager> = LazyLock::new(ConnectionManager::new);

// ---------------------------------------------------------------------------
// ルーター
// ---------------------------------------------------------------------------

/// WebSocket 関連のルーターを返す
pub fn router() -> Router<crate::AppState> {
    Router::new().route("/ws", get(ws_handler))
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

/// WebSocket 接続のクエリパラメータ
#[derive(Debug, Deserialize)]
struct WsQuery {
    token: String,
}

/// GET /ws?token=<jwt> — WebSocket アップグレード
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::AppState>,
    Query(query): Query<WsQuery>,
) -> Response {
    match crate::jwt::verify_token(&query.token, &state.jwt_secret) {
        Ok(claims) => ws.on_upgrade(move |socket| handle_socket(socket, claims.sub)),
        Err(_) => (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    }
}

/// WebSocket 接続のメインループ
async fn handle_socket(mut socket: WebSocket, user_id: String) {
    // クライアントへのイベント送信用チャネル
    let (tx, mut rx) = tokio::sync::mpsc::channel::<WsEvent>(32);

    // 購読中のグループを追跡
    let mut subscribed_groups: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            // クライアントからのメッセージ受信
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(cmd) = serde_json::from_str::<WsCommand>(&text) {
                            match cmd {
                                WsCommand::SubscribeGroup { group_id } => {
                                    // 既に購読済みならスキップ
                                    if subscribed_groups.contains(&group_id) {
                                        continue;
                                    }

                                    // グループの broadcast に購読
                                    let sender = CONNECTIONS
                                        .get_or_create_group(&group_id)
                                        .await;
                                    let mut broadcast_rx = sender.subscribe();
                                    let fwd_tx = tx.clone();

                                    tokio::spawn(async move {
                                        while let Ok(event) = broadcast_rx.recv().await {
                                            if fwd_tx.send(event).await.is_err() {
                                                break;
                                            }
                                        }
                                    });

                                    subscribed_groups.push(group_id.clone());

                                    // オンライン通知をグループに送信
                                    CONNECTIONS
                                        .broadcast_to_group(
                                            &group_id,
                                            WsEvent::Presence {
                                                user_id: user_id.clone(),
                                                status: "online".to_string(),
                                            },
                                        )
                                        .await;
                                }
                                WsCommand::SetPresence { status } => {
                                    for gid in &subscribed_groups {
                                        CONNECTIONS
                                            .broadcast_to_group(
                                                gid,
                                                WsEvent::Presence {
                                                    user_id: user_id.clone(),
                                                    status: status.clone(),
                                                },
                                            )
                                            .await;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            // クライアントへのイベント転送
            Some(event) = rx.recv() => {
                let json = serde_json::to_string(&event).unwrap();
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    // 切断: 全購読グループにオフライン通知
    for gid in &subscribed_groups {
        CONNECTIONS
            .broadcast_to_group(
                gid,
                WsEvent::Presence {
                    user_id: user_id.clone(),
                    status: "offline".to_string(),
                },
            )
            .await;
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_event_session_started_serialization() {
        let event = WsEvent::SessionStarted {
            group_id: "g1".into(),
            session_id: "s1".into(),
            started_by: "u1".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"SessionStarted\""));
        assert!(json.contains("\"group_id\":\"g1\""));

        let parsed: WsEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            WsEvent::SessionStarted {
                group_id,
                session_id,
                started_by,
            } => {
                assert_eq!(group_id, "g1");
                assert_eq!(session_id, "s1");
                assert_eq!(started_by, "u1");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_ws_event_presence_serialization() {
        let event = WsEvent::Presence {
            user_id: "u1".into(),
            status: "online".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"Presence\""));

        let parsed: WsEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            WsEvent::Presence { user_id, status } => {
                assert_eq!(user_id, "u1");
                assert_eq!(status, "online");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_ws_event_peer_joined_serialization() {
        let event = WsEvent::PeerJoined {
            session_id: "s1".into(),
            user_id: "u1".into(),
            addr: "192.168.1.1:5000".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: WsEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            WsEvent::PeerJoined {
                session_id,
                user_id,
                addr,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(user_id, "u1");
                assert_eq!(addr, "192.168.1.1:5000");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_ws_event_peer_left_serialization() {
        let event = WsEvent::PeerLeft {
            session_id: "s1".into(),
            user_id: "u1".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: WsEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            WsEvent::PeerLeft {
                session_id,
                user_id,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(user_id, "u1");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_ws_command_subscribe_group() {
        let cmd = WsCommand::SubscribeGroup {
            group_id: "g1".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"SubscribeGroup\""));

        let parsed: WsCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            WsCommand::SubscribeGroup { group_id } => {
                assert_eq!(group_id, "g1");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_ws_command_set_presence() {
        let cmd = WsCommand::SetPresence {
            status: "offline".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"SetPresence\""));

        let parsed: WsCommand = serde_json::from_str(&json).unwrap();
        match parsed {
            WsCommand::SetPresence { status } => {
                assert_eq!(status, "offline");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_ws_command_from_raw_json() {
        let json = r#"{"type":"SubscribeGroup","group_id":"my-group"}"#;
        let cmd: WsCommand = serde_json::from_str(json).unwrap();
        match cmd {
            WsCommand::SubscribeGroup { group_id } => assert_eq!(group_id, "my-group"),
            _ => panic!("unexpected variant"),
        }
    }

    #[tokio::test]
    async fn test_connection_manager_get_or_create_group() {
        let cm = ConnectionManager::new();
        let tx1 = cm.get_or_create_group("group-1").await;
        let tx2 = cm.get_or_create_group("group-1").await;
        // 同じグループなので同じチャネル
        assert_eq!(tx1.receiver_count(), tx2.receiver_count());
    }

    #[tokio::test]
    async fn test_connection_manager_broadcast_to_group() {
        let cm = ConnectionManager::new();
        let tx = cm.get_or_create_group("group-1").await;
        let mut rx = tx.subscribe();

        cm.broadcast_to_group(
            "group-1",
            WsEvent::Presence {
                user_id: "u1".into(),
                status: "online".into(),
            },
        )
        .await;

        let event = rx.recv().await.unwrap();
        match event {
            WsEvent::Presence { user_id, status } => {
                assert_eq!(user_id, "u1");
                assert_eq!(status, "online");
            }
            _ => panic!("unexpected event"),
        }
    }

    #[tokio::test]
    async fn test_connection_manager_broadcast_to_nonexistent_group() {
        let cm = ConnectionManager::new();
        // 存在しないグループへの broadcast はエラーにならない
        cm.broadcast_to_group(
            "no-such-group",
            WsEvent::Presence {
                user_id: "u1".into(),
                status: "offline".into(),
            },
        )
        .await;
    }

    #[tokio::test]
    async fn test_connection_manager_multiple_groups() {
        let cm = ConnectionManager::new();
        let tx1 = cm.get_or_create_group("group-a").await;
        let tx2 = cm.get_or_create_group("group-b").await;
        let mut rx1 = tx1.subscribe();
        let mut rx2 = tx2.subscribe();

        // group-a にだけ送信
        cm.broadcast_to_group(
            "group-a",
            WsEvent::Presence {
                user_id: "u1".into(),
                status: "online".into(),
            },
        )
        .await;

        // group-a は受信できる
        let event = rx1.recv().await.unwrap();
        match event {
            WsEvent::Presence { user_id, .. } => assert_eq!(user_id, "u1"),
            _ => panic!("unexpected event"),
        }

        // group-b には何も来ていない
        assert!(rx2.try_recv().is_err());
    }
}
