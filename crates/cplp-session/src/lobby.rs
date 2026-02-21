//! ロビーサーバー HTTP + WebSocket クライアント
//!
//! REQ-NET-002: ロビー経由のピア発見
//!
//! HTTP API でセッション CRUD、WebSocket でリアルタイムイベント受信。

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::signaling::{
    CreateSessionResponse, GroupInfo, JoinSessionResponse, LeaveSessionResponse, LobbyCommand,
    LobbyConfig, LobbyEvent, PeersResponse,
};

/// ロビーサーバークライアント
pub struct LobbyClient {
    config: LobbyConfig,
    http: reqwest::Client,
    /// WebSocket イベント受信チャネル（take_event_rx で取得）
    event_rx: Option<mpsc::UnboundedReceiver<LobbyEvent>>,
    /// WebSocket コマンド送信チャネル
    cmd_tx: Option<mpsc::UnboundedSender<LobbyCommand>>,
}

impl LobbyClient {
    pub fn new(config: LobbyConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            event_rx: None,
            cmd_tx: None,
        }
    }

    pub fn config(&self) -> &LobbyConfig {
        &self.config
    }

    // -----------------------------------------------------------------------
    // HTTP API
    // -----------------------------------------------------------------------

    /// GET /groups — ユーザーが所属するグループ一覧
    pub async fn list_groups(&self) -> anyhow::Result<Vec<GroupInfo>> {
        let url = format!("{}/groups", self.config.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    /// POST /groups/{group_id}/sessions — セッションを作成
    pub async fn create_session(&self, group_id: &str) -> anyhow::Result<CreateSessionResponse> {
        let url = format!("{}/groups/{}/sessions", self.config.base_url, group_id);
        let body = serde_json::json!({ "addr": self.config.local_addr.to_string() });
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    /// POST /sessions/{id}/join — セッションに参加
    pub async fn join_session(&self, session_id: &str) -> anyhow::Result<JoinSessionResponse> {
        let id_part = session_id.strip_prefix("sessions:").unwrap_or(session_id);
        let url = format!("{}/sessions/{}/join", self.config.base_url, id_part);
        let body = serde_json::json!({ "addr": self.config.local_addr.to_string() });
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    /// GET /sessions/{id}/peers — ピア一覧を取得
    pub async fn get_peers(&self, session_id: &str) -> anyhow::Result<PeersResponse> {
        let id_part = session_id.strip_prefix("sessions:").unwrap_or(session_id);
        let url = format!("{}/sessions/{}/peers", self.config.base_url, id_part);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    /// POST /sessions/{id}/leave — セッションから離脱
    pub async fn leave_session(&self, session_id: &str) -> anyhow::Result<LeaveSessionResponse> {
        let id_part = session_id.strip_prefix("sessions:").unwrap_or(session_id);
        let url = format!("{}/sessions/{}/leave", self.config.base_url, id_part);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    // -----------------------------------------------------------------------
    // WebSocket
    // -----------------------------------------------------------------------

    /// WebSocket 接続を確立しバックグラウンドタスクを起動
    ///
    /// イベントは `take_event_rx()` で取得した receiver から受信する。
    pub async fn connect_ws(&mut self) -> anyhow::Result<()> {
        let ws_url = build_ws_url(&self.config.base_url, &self.config.token)?;

        let (ws_stream, _response) = tokio_tungstenite::connect_async(&ws_url).await?;
        let (mut ws_write, mut ws_read) = ws_stream.split();

        // イベント受信チャネル
        let (event_tx, event_rx) = mpsc::unbounded_channel::<LobbyEvent>();
        self.event_rx = Some(event_rx);

        // コマンド送信チャネル
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<LobbyCommand>();
        self.cmd_tx = Some(cmd_tx);

        // バックグラウンド: WS → event_tx
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = ws_read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(event) = serde_json::from_str::<LobbyEvent>(&text) {
                                    if event_tx.send(event).is_err() {
                                        break;
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break,
                            _ => {}
                        }
                    }
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(cmd) => {
                                let json = serde_json::to_string(&cmd).unwrap();
                                if ws_write.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        tracing::info!("WebSocket 接続完了: {}", ws_url);
        Ok(())
    }

    /// グループの WebSocket イベントを購読
    pub fn subscribe_group(&self, group_id: &str) -> anyhow::Result<()> {
        let cmd_tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket 未接続です。connect_ws() を先に呼んでください"))?;
        cmd_tx.send(LobbyCommand::SubscribeGroup {
            group_id: group_id.to_string(),
        })?;
        Ok(())
    }

    /// イベント受信チャネルを取得（一度だけ呼べる）
    pub fn take_event_rx(&mut self) -> Option<mpsc::UnboundedReceiver<LobbyEvent>> {
        self.event_rx.take()
    }
}

/// HTTP URL を WebSocket URL に変換して token クエリパラメータを付与
fn build_ws_url(base_url: &str, token: &str) -> anyhow::Result<String> {
    let mut url = url::Url::parse(base_url)?;

    // http → ws, https → wss
    match url.scheme() {
        "http" => url.set_scheme("ws").ok(),
        "https" => url.set_scheme("wss").ok(),
        _ => None,
    };

    url.set_path("/ws");
    url.query_pairs_mut().append_pair("token", token);

    Ok(url.to_string())
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_ws_url_http() {
        let url = build_ws_url("http://localhost:3000", "my-token").unwrap();
        assert_eq!(url, "ws://localhost:3000/ws?token=my-token");
    }

    #[test]
    fn test_build_ws_url_https() {
        let url = build_ws_url("https://lobby.example.com", "jwt123").unwrap();
        assert_eq!(url, "wss://lobby.example.com/ws?token=jwt123");
    }

    #[test]
    fn test_lobby_client_new() {
        let config = LobbyConfig {
            base_url: "http://localhost:3000".into(),
            token: "test-token".into(),
            local_addr: "[::1]:5000".parse().unwrap(),
        };
        let client = LobbyClient::new(config);
        assert_eq!(client.config().base_url, "http://localhost:3000");
    }
}
