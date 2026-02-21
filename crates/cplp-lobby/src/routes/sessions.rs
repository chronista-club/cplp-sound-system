//! セッション管理 API
//!
//! REQ-LOBBY-004: P2P セッションのライフサイクル管理

use axum::Router;
use axum::extract::{Json, Path, State};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;

use crate::auth::AuthUser;
use crate::db::to_record_id;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// リクエスト / レスポンス型（HTTP JSON）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    addr: String,
}

#[derive(Debug, Serialize)]
struct CreateSessionResponse {
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct JoinSessionRequest {
    addr: String,
}

#[derive(Debug, Serialize)]
struct JoinSessionResponse {
    status: String,
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Serialize)]
struct PeerInfo {
    user_id: String,
    addr: String,
}

#[derive(Debug, Serialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Serialize)]
struct LeaveSessionResponse {
    status: String,
}

// ---------------------------------------------------------------------------
// SurrealDB クエリ結果の型
//
// SurrealDB の record / datetime 型は serde_json::Value に変換できないため、
// 専用の struct を定義して take() で型安全にデシリアライズする。
// ---------------------------------------------------------------------------

/// CREATE sessions の結果行
#[derive(Debug, Deserialize)]
struct SessionRow {
    id: RecordId,
    #[allow(dead_code)]
    status: String,
}

/// SELECT count() ... GROUP ALL の結果行
#[derive(Debug, Deserialize)]
struct CountRow {
    total: i64,
}

/// SELECT in AS user_id, addr FROM session_peers の結果行
#[derive(Debug, Deserialize)]
struct PeerRow {
    user_id: RecordId,
    addr: String,
}

/// SELECT status FROM sessions の結果行
#[derive(Debug, Deserialize)]
struct StatusRow {
    status: String,
}

// ---------------------------------------------------------------------------
// ルーター
// ---------------------------------------------------------------------------

/// セッション関連のルーターを返す
pub fn router() -> Router<crate::AppState> {
    Router::new()
        .route("/groups/{group_id}/sessions", post(create_session))
        .route("/sessions/{id}/join", post(join_session))
        .route("/sessions/{id}/peers", get(get_peers))
        .route("/sessions/{id}/leave", post(leave_session))
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

/// POST /groups/{group_id}/sessions — セッションを開始する
async fn create_session(
    State(state): State<crate::AppState>,
    auth_user: AuthUser,
    Path(group_id): Path<String>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, AppError> {
    let db = &state.db;
    let group_id_str = group_id.clone();

    // セッションを作成
    let mut result = db
        .query(
            "CREATE sessions SET \
             group_id = type::thing('groups', $group_id), \
             started_by = type::thing($user_id), \
             status = 'waiting'",
        )
        .bind(("group_id", group_id))
        .bind(("user_id", auth_user.user_id.clone()))
        .await?;

    let sessions: Vec<SessionRow> = result.take(0)?;
    let session = sessions
        .first()
        .ok_or_else(|| anyhow::anyhow!("failed to create session"))?;
    let session_id = session.id.to_string();

    // 作成者を最初のピアとして追加
    db.query("RELATE $user_id->session_peers->$session_id SET addr = $addr")
        .bind(("user_id", to_record_id(&auth_user.user_id)?))
        .bind(("session_id", to_record_id(&session_id)?))
        .bind(("addr", body.addr))
        .await?
        .check()?;

    // WebSocket 経由でグループにセッション開始を通知
    crate::ws::CONNECTIONS
        .broadcast_to_group(
            &group_id_str,
            crate::ws::WsEvent::SessionStarted {
                group_id: group_id_str.clone(),
                session_id: session_id.clone(),
                started_by: auth_user.user_id,
            },
        )
        .await;

    Ok(Json(CreateSessionResponse {
        id: session_id,
        status: "waiting".to_string(),
    }))
}

/// POST /sessions/{id}/join — セッションに参加する
async fn join_session(
    State(state): State<crate::AppState>,
    auth_user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<JoinSessionRequest>,
) -> Result<Json<JoinSessionResponse>, AppError> {
    let db = &state.db;
    let session_id = format!("sessions:{id}");
    let peer_addr = body.addr.clone();

    // 既に参加済みかチェック（二重参加防止）
    let mut existing = db
        .query(
            "SELECT count() AS total FROM session_peers \
             WHERE in = type::thing($user_id) AND out = type::thing($session_id) GROUP ALL",
        )
        .bind(("user_id", auth_user.user_id.clone()))
        .bind(("session_id", session_id.clone()))
        .await?;
    let existing_counts: Vec<CountRow> = existing.take(0)?;
    if existing_counts.first().is_some_and(|c| c.total > 0) {
        // 既に参加済み — アドレスだけ更新
        db.query(
            "UPDATE session_peers SET addr = $addr \
             WHERE in = type::thing($user_id) AND out = type::thing($session_id)",
        )
        .bind(("user_id", auth_user.user_id.clone()))
        .bind(("session_id", session_id.clone()))
        .bind(("addr", body.addr.clone()))
        .await?;
    } else {
        // 新規参加
        db.query("RELATE $user_id->session_peers->$session_id SET addr = $addr")
            .bind(("user_id", to_record_id(&auth_user.user_id)?))
            .bind(("session_id", to_record_id(&session_id)?))
            .bind(("addr", body.addr.clone()))
            .await?
            .check()?;
    }

    // ピア数を確認
    let mut count_result = db
        .query(
            "SELECT count() AS total FROM session_peers \
             WHERE out = type::thing($session_id) GROUP ALL",
        )
        .bind(("session_id", session_id.clone()))
        .await?;

    let counts: Vec<CountRow> = count_result.take(0)?;
    let peer_count = counts.first().map(|c| c.total).unwrap_or(0);

    // 2人以上なら status を "active" に更新
    let status = if peer_count >= 2 {
        db.query("UPDATE type::thing($session_id) SET status = 'active'")
            .bind(("session_id", session_id.clone()))
            .await?;
        "active".to_string()
    } else {
        "waiting".to_string()
    };

    // ピア一覧を取得
    let peers = fetch_peers(db, &session_id).await?;

    // WebSocket 経由で PeerJoined を broadcast
    let group_id = fetch_group_id(db, &session_id).await?;
    crate::ws::CONNECTIONS
        .broadcast_to_group(
            &group_id,
            crate::ws::WsEvent::PeerJoined {
                session_id: session_id.clone(),
                user_id: auth_user.user_id,
                addr: peer_addr,
            },
        )
        .await;

    Ok(Json(JoinSessionResponse { status, peers }))
}

/// GET /sessions/{id}/peers — ピア一覧を取得する
async fn get_peers(
    State(state): State<crate::AppState>,
    _auth_user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<PeersResponse>, AppError> {
    let session_id = format!("sessions:{id}");
    let peers = fetch_peers(&state.db, &session_id).await?;
    Ok(Json(PeersResponse { peers }))
}

/// POST /sessions/{id}/leave — セッションから離脱する
async fn leave_session(
    State(state): State<crate::AppState>,
    auth_user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<LeaveSessionResponse>, AppError> {
    let db = &state.db;
    let session_id = format!("sessions:{id}");

    // WebSocket 経由で PeerLeft を broadcast（削除前に group_id を取得）
    let group_id = fetch_group_id(db, &session_id).await?;
    crate::ws::CONNECTIONS
        .broadcast_to_group(
            &group_id,
            crate::ws::WsEvent::PeerLeft {
                session_id: session_id.clone(),
                user_id: auth_user.user_id.clone(),
            },
        )
        .await;

    // このユーザーの session_peers リレーションを削除
    db.query(
        "DELETE session_peers \
         WHERE in = type::thing($user_id) AND out = type::thing($session_id)",
    )
    .bind(("user_id", auth_user.user_id))
    .bind(("session_id", session_id.clone()))
    .await?;

    // 残りのピア数を確認
    let mut count_result = db
        .query(
            "SELECT count() AS total FROM session_peers \
             WHERE out = type::thing($session_id) GROUP ALL",
        )
        .bind(("session_id", session_id.clone()))
        .await?;

    let counts: Vec<CountRow> = count_result.take(0)?;
    let remaining = counts.first().map(|c| c.total).unwrap_or(0);

    // ピアがいなくなったらセッションを終了
    if remaining == 0 {
        db.query("UPDATE type::thing($session_id) SET status = 'ended'")
            .bind(("session_id", session_id.clone()))
            .await?;
    }

    // 最終的なステータスを取得
    let mut status_result = db
        .query("SELECT status FROM type::thing($session_id)")
        .bind(("session_id", session_id))
        .await?;

    let rows: Vec<StatusRow> = status_result.take(0)?;
    let status = rows
        .first()
        .map(|r| r.status.clone())
        .unwrap_or_else(|| "ended".to_string());

    Ok(Json(LeaveSessionResponse { status }))
}

// ---------------------------------------------------------------------------
// ヘルパー
// ---------------------------------------------------------------------------

/// セッションの group_id を取得する
async fn fetch_group_id(db: &crate::db::Db, session_id: &str) -> anyhow::Result<String> {
    #[derive(Debug, serde::Deserialize)]
    struct GroupIdRow {
        group_id: RecordId,
    }

    let mut result = db
        .query("SELECT group_id FROM ONLY type::thing($session_id)")
        .bind(("session_id", session_id.to_string()))
        .await?;

    let row: Option<GroupIdRow> = result.take(0)?;
    Ok(row.map(|r| r.group_id.to_string()).unwrap_or_default())
}

/// セッションのピア一覧を取得する
async fn fetch_peers(db: &crate::db::Db, session_id: &str) -> anyhow::Result<Vec<PeerInfo>> {
    let mut result = db
        .query(
            "SELECT in AS user_id, addr FROM session_peers \
             WHERE out = type::thing($session_id)",
        )
        .bind(("session_id", session_id.to_string()))
        .await?;

    let rows: Vec<PeerRow> = result.take(0)?;
    let peers = rows
        .into_iter()
        .map(|row| PeerInfo {
            user_id: row.user_id.to_string(),
            addr: row.addr,
        })
        .collect();

    Ok(peers)
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use axum::http::{self, StatusCode};

    /// テスト用のサーバー・DB・トークンを準備する
    async fn test_app() -> (axum_test::TestServer, crate::db::Db, String) {
        let db = crate::db::init_test_db().await.unwrap();
        let state = crate::AppState {
            db: db.clone(),
            oauth: crate::auth::OAuthConfig {
                github: None,
                google: None,
                discord: None,
            },
            jwt_secret: "test-secret".to_string(),
        };

        // テストユーザーを作成
        db.query(
            "CREATE users:testuser SET \
             name = 'Test User', \
             email = 'test@example.com', \
             oauth_provider = 'github', \
             oauth_id = '123'",
        )
        .await
        .unwrap()
        .check()
        .unwrap();

        // テストグループを作成
        db.query(
            "CREATE groups:testgroup SET \
             name = 'Test Band', \
             created_by = users:testuser",
        )
        .await
        .unwrap()
        .check()
        .unwrap();

        let token = crate::jwt::create_token("users:testuser", "test-secret").unwrap();
        let server = axum_test::TestServer::new(crate::create_router(state)).unwrap();
        (server, db, token)
    }

    /// Bearer 認証ヘッダーを作成する
    fn auth_header(token: &str) -> (http::header::HeaderName, http::HeaderValue) {
        (
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        )
    }

    #[tokio::test]
    async fn test_create_session() {
        let (server, _db, token) = test_app().await;
        let (name, value) = auth_header(&token);

        let res = server
            .post("/groups/testgroup/sessions")
            .add_header(name, value)
            .json(&serde_json::json!({ "addr": "192.168.1.1:5000" }))
            .await;

        res.assert_status_ok();
        let body: serde_json::Value = res.json();
        assert!(body["id"].as_str().unwrap().starts_with("sessions:"));
        assert_eq!(body["status"], "waiting");
    }

    #[tokio::test]
    async fn test_create_session_requires_auth() {
        let (server, _db, _token) = test_app().await;

        let res = server
            .post("/groups/testgroup/sessions")
            .json(&serde_json::json!({ "addr": "192.168.1.1:5000" }))
            .await;

        res.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_join_session_becomes_active() {
        let (server, db, token) = test_app().await;

        // セッションを作成
        let (name, value) = auth_header(&token);
        let res = server
            .post("/groups/testgroup/sessions")
            .add_header(name, value)
            .json(&serde_json::json!({ "addr": "192.168.1.1:5000" }))
            .await;
        res.assert_status_ok();
        let session_id_full = res.json::<serde_json::Value>()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let id_part = session_id_full.strip_prefix("sessions:").unwrap();

        // 2人目のユーザーを作成
        db.query(
            "CREATE users:user2 SET \
             name = 'User Two', \
             email = 'user2@example.com', \
             oauth_provider = 'github', \
             oauth_id = '456'",
        )
        .await
        .unwrap()
        .check()
        .unwrap();
        let token2 = crate::jwt::create_token("users:user2", "test-secret").unwrap();
        let (name2, value2) = auth_header(&token2);

        // セッションに参加
        let res = server
            .post(&format!("/sessions/{id_part}/join"))
            .add_header(name2, value2)
            .json(&serde_json::json!({ "addr": "192.168.1.2:5001" }))
            .await;

        res.assert_status_ok();
        let body: serde_json::Value = res.json();
        assert_eq!(body["status"], "active");
        assert_eq!(body["peers"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_get_peers() {
        let (server, _db, token) = test_app().await;

        // セッションを作成
        let (name, value) = auth_header(&token);
        let res = server
            .post("/groups/testgroup/sessions")
            .add_header(name.clone(), value.clone())
            .json(&serde_json::json!({ "addr": "192.168.1.1:5000" }))
            .await;
        res.assert_status_ok();
        let id_part = res.json::<serde_json::Value>()["id"]
            .as_str()
            .unwrap()
            .strip_prefix("sessions:")
            .unwrap()
            .to_string();

        // ピア一覧を取得
        let res = server
            .get(&format!("/sessions/{id_part}/peers"))
            .add_header(name, value)
            .await;

        res.assert_status_ok();
        let body: serde_json::Value = res.json();
        let peers = body["peers"].as_array().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0]["addr"], "192.168.1.1:5000");
    }

    #[tokio::test]
    async fn test_leave_session_ends_when_empty() {
        let (server, _db, token) = test_app().await;

        // セッションを作成
        let (name, value) = auth_header(&token);
        let res = server
            .post("/groups/testgroup/sessions")
            .add_header(name.clone(), value.clone())
            .json(&serde_json::json!({ "addr": "192.168.1.1:5000" }))
            .await;
        res.assert_status_ok();
        let id_part = res.json::<serde_json::Value>()["id"]
            .as_str()
            .unwrap()
            .strip_prefix("sessions:")
            .unwrap()
            .to_string();

        // セッションから離脱（最後のピア）
        let res = server
            .post(&format!("/sessions/{id_part}/leave"))
            .add_header(name, value)
            .await;

        res.assert_status_ok();
        let body: serde_json::Value = res.json();
        assert_eq!(body["status"], "ended");
    }
}
