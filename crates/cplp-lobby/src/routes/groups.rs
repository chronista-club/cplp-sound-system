//! グループ CRUD API
//!
//! REQ-LOBBY-003: グループ管理

use axum::Router;
use axum::extract::{Json, Path, State};
use axum::routing::{delete, get, post};
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::to_record_id;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// リクエスト / レスポンス型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroupResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemberInfo {
    pub user_id: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroupDetailResponse {
    pub id: String,
    pub name: String,
    pub members: Vec<MemberInfo>,
}

#[derive(Debug, Deserialize)]
pub struct InviteRequest {
    pub user_id: String,
}

// ---------------------------------------------------------------------------
// ルーター
// ---------------------------------------------------------------------------

/// グループ関連のルーターを返す
pub fn router() -> Router<crate::AppState> {
    Router::new()
        .route("/groups", post(create_group).get(list_groups))
        .route("/groups/{id}", get(get_group))
        .route("/groups/{id}/invite", post(invite_member))
        .route("/groups/{id}/members/{uid}", delete(remove_member))
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

/// POST /groups - グループを作成
async fn create_group(
    State(state): State<crate::AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGroupRequest>,
) -> Result<Json<GroupResponse>, AppError> {
    let user_id = auth.user_id;
    let name = body.name;

    // グループを作成
    let mut result = state
        .db
        .query(
            "CREATE groups SET name = $name, created_by = type::thing($created_by) \
             RETURN <string> id AS id, name",
        )
        .bind(("name", name.clone()))
        .bind(("created_by", user_id.clone()))
        .await?;

    let created: Vec<serde_json::Value> = result.take(0)?;
    let group = created
        .first()
        .ok_or_else(|| anyhow::anyhow!("グループの作成に失敗しました"))?;

    let group_id = group["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("グループIDの取得に失敗しました"))?
        .to_string();

    // 作成者を owner として RELATE
    state
        .db
        .query("RELATE $user_id->member_of->$group_id SET role = 'owner'")
        .bind(("user_id", to_record_id(&user_id)))
        .bind(("group_id", to_record_id(&group_id)))
        .await?
        .check()?;

    Ok(Json(GroupResponse { id: group_id, name }))
}

/// GET /groups - ユーザーが所属するグループ一覧
async fn list_groups(
    State(state): State<crate::AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<GroupResponse>>, AppError> {
    let mut result = state
        .db
        .query(
            "SELECT <string> id AS id, name FROM groups \
             WHERE <-member_of<-users CONTAINS type::thing($user_id)",
        )
        .bind(("user_id", auth.user_id))
        .await?;

    let rows: Vec<serde_json::Value> = result.take(0)?;

    let groups = rows
        .iter()
        .filter_map(|row| {
            Some(GroupResponse {
                id: row["id"].as_str()?.to_string(),
                name: row["name"].as_str()?.to_string(),
            })
        })
        .collect();

    Ok(Json(groups))
}

/// GET /groups/{id} - グループ詳細（メンバー一覧付き）
async fn get_group(
    State(state): State<crate::AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GroupDetailResponse>, AppError> {
    let group_id = format!("groups:{id}");

    // グループ情報を取得
    let mut result = state
        .db
        .query("SELECT <string> id AS id, name FROM ONLY type::thing($group_id)")
        .bind(("group_id", group_id.clone()))
        .await?;

    let group: Option<serde_json::Value> = result.take(0)?;
    let group = group.ok_or_else(|| anyhow::anyhow!("グループが見つかりません"))?;

    let name = group["name"].as_str().unwrap_or_default().to_string();

    // メンバー一覧を取得（member_of リレーション経由）
    let mut result = state
        .db
        .query(
            "SELECT <string> in AS user_id, in.name AS name, role \
             FROM member_of WHERE out = type::thing($group_id)",
        )
        .bind(("group_id", group_id.clone()))
        .await?;

    let member_rows: Vec<serde_json::Value> = result.take(0)?;

    let members = member_rows
        .iter()
        .filter_map(|row| {
            Some(MemberInfo {
                user_id: row["user_id"].as_str()?.to_string(),
                name: row["name"].as_str().unwrap_or_default().to_string(),
                role: row["role"].as_str()?.to_string(),
            })
        })
        .collect();

    Ok(Json(GroupDetailResponse {
        id: group_id,
        name,
        members,
    }))
}

/// POST /groups/{id}/invite - メンバーを招待
async fn invite_member(
    State(state): State<crate::AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<InviteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let group_id = format!("groups:{id}");

    // 呼び出し元が owner または admin かチェック
    let mut result = state
        .db
        .query(
            "SELECT role FROM member_of \
             WHERE in = type::thing($user_id) AND out = type::thing($group_id)",
        )
        .bind(("user_id", auth.user_id))
        .bind(("group_id", group_id.clone()))
        .await?;

    let roles: Vec<serde_json::Value> = result.take(0)?;
    let role = roles.first().and_then(|r| r["role"].as_str()).unwrap_or("");

    if role != "owner" && role != "admin" {
        return Err(anyhow::anyhow!("owner または admin のみ招待できます").into());
    }

    // メンバーを追加
    state
        .db
        .query("RELATE $user_id->member_of->$group_id SET role = 'member'")
        .bind(("user_id", to_record_id(&body.user_id)))
        .bind(("group_id", to_record_id(&group_id)))
        .await?
        .check()?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// DELETE /groups/{id}/members/{uid} - メンバーを削除
async fn remove_member(
    State(state): State<crate::AppState>,
    auth: AuthUser,
    Path((id, uid)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let group_id = format!("groups:{id}");
    let target_user_id = format!("users:{uid}");

    // 呼び出し元が owner かチェック
    let mut result = state
        .db
        .query(
            "SELECT role FROM member_of \
             WHERE in = type::thing($user_id) AND out = type::thing($group_id)",
        )
        .bind(("user_id", auth.user_id))
        .bind(("group_id", group_id.clone()))
        .await?;

    let roles: Vec<serde_json::Value> = result.take(0)?;
    let role = roles.first().and_then(|r| r["role"].as_str()).unwrap_or("");

    if role != "owner" {
        return Err(anyhow::anyhow!("owner のみメンバーを削除できます").into());
    }

    // member_of リレーションを削除
    state
        .db
        .query("DELETE member_of WHERE in = type::thing($target) AND out = type::thing($group_id)")
        .bind(("target", target_user_id))
        .bind(("group_id", group_id))
        .await?
        .check()?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{self, StatusCode};
    use axum_test::TestServer;

    /// テスト用のアプリケーションとJWTトークンを準備
    async fn test_app() -> (TestServer, String) {
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

        // テスト用ユーザーを作成
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

        let token = crate::jwt::create_token("users:testuser", "test-secret").unwrap();
        let server = TestServer::new(crate::create_router(state)).unwrap();
        (server, token)
    }

    fn auth_header(token: &str) -> (http::HeaderName, http::HeaderValue) {
        (
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        )
    }

    #[tokio::test]
    async fn create_group_succeeds() {
        let (server, token) = test_app().await;
        let (h_name, h_val) = auth_header(&token);

        let res = server
            .post("/groups")
            .add_header(h_name, h_val)
            .json(&serde_json::json!({ "name": "My Band" }))
            .await;

        res.assert_status_ok();
        let body: GroupResponse = res.json();
        assert!(body.id.starts_with("groups:"));
        assert_eq!(body.name, "My Band");
    }

    #[tokio::test]
    async fn create_group_requires_auth() {
        let (server, _token) = test_app().await;

        let res = server
            .post("/groups")
            .json(&serde_json::json!({ "name": "No Auth" }))
            .await;

        res.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_groups_returns_user_groups() {
        let (server, token) = test_app().await;
        let (h_name, h_val) = auth_header(&token);

        // グループを作成
        server
            .post("/groups")
            .add_header(h_name.clone(), h_val.clone())
            .json(&serde_json::json!({ "name": "Band A" }))
            .await
            .assert_status_ok();

        server
            .post("/groups")
            .add_header(h_name.clone(), h_val.clone())
            .json(&serde_json::json!({ "name": "Band B" }))
            .await
            .assert_status_ok();

        // 一覧取得
        let res = server.get("/groups").add_header(h_name, h_val).await;

        res.assert_status_ok();
        let groups: Vec<GroupResponse> = res.json();
        assert_eq!(groups.len(), 2);

        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"Band A"));
        assert!(names.contains(&"Band B"));
    }

    #[tokio::test]
    async fn get_group_detail_with_members() {
        let (server, token) = test_app().await;
        let (h_name, h_val) = auth_header(&token);

        // グループを作成
        let res = server
            .post("/groups")
            .add_header(h_name.clone(), h_val.clone())
            .json(&serde_json::json!({ "name": "Detail Band" }))
            .await;
        res.assert_status_ok();
        let created: GroupResponse = res.json();

        // グループIDからパスを構築（"groups:xxx" -> "xxx"）
        let id_suffix = created.id.strip_prefix("groups:").unwrap();

        let res = server
            .get(&format!("/groups/{id_suffix}"))
            .add_header(h_name, h_val)
            .await;

        res.assert_status_ok();
        let detail: GroupDetailResponse = res.json();
        assert_eq!(detail.name, "Detail Band");
        assert_eq!(detail.members.len(), 1);
        assert_eq!(detail.members[0].user_id, "users:testuser");
        assert_eq!(detail.members[0].role, "owner");
    }

    #[tokio::test]
    async fn invite_member_and_list() {
        let (server, token) = test_app().await;
        let (h_name, h_val) = auth_header(&token);

        // グループを作成
        let res = server
            .post("/groups")
            .add_header(h_name.clone(), h_val.clone())
            .json(&serde_json::json!({ "name": "Invite Test" }))
            .await;
        res.assert_status_ok();
        let created: GroupResponse = res.json();
        let id_suffix = created.id.strip_prefix("groups:").unwrap();

        // メンバーを招待（owner なので成功するはず）
        let res = server
            .post(&format!("/groups/{id_suffix}/invite"))
            .add_header(h_name.clone(), h_val.clone())
            .json(&serde_json::json!({ "user_id": "users:invited1" }))
            .await;
        res.assert_status_ok();

        // グループ詳細でメンバーを確認
        let res = server
            .get(&format!("/groups/{id_suffix}"))
            .add_header(h_name, h_val)
            .await;
        res.assert_status_ok();
        let detail: GroupDetailResponse = res.json();
        assert_eq!(detail.members.len(), 2);
    }
}
