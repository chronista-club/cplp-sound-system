//! SurrealDB 接続管理
//!
//! REQ-LOBBY-001: ロビーサーバーのデータ永続化

use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// SurrealDB クライアント型エイリアス
pub type Db = Surreal<Any>;

/// SurrealDB に接続し、スキーマを初期化する
///
/// 環境変数:
/// - `SURREAL_URL`: 接続先 (デフォルト: `mem://`)
/// - `SURREAL_NS`: ネームスペース (デフォルト: `cplp`)
/// - `SURREAL_DB`: データベース名 (デフォルト: `lobby`)
pub async fn init_db() -> anyhow::Result<Db> {
    let url = std::env::var("SURREAL_URL").unwrap_or_else(|_| "mem://".to_string());
    let ns = std::env::var("SURREAL_NS").unwrap_or_else(|_| "cplp".to_string());
    let db_name = std::env::var("SURREAL_DB").unwrap_or_else(|_| "lobby".to_string());

    let db = surrealdb::engine::any::connect(&url).await?;
    db.use_ns(&ns).use_db(&db_name).await?;

    init_schema(&db).await?;
    tracing::info!("SurrealDB connected: {}/{}/{}", url, ns, db_name);
    Ok(db)
}

/// テスト用: インメモリ DB を初期化
pub async fn init_test_db() -> anyhow::Result<Db> {
    let db = surrealdb::engine::any::connect("mem://").await?;
    db.use_ns("test").use_db("test").await?;
    init_schema(&db).await?;
    Ok(db)
}

/// スキーマ定義を実行
async fn init_schema(db: &Db) -> anyhow::Result<()> {
    let schema = include_str!("schema.surql");
    db.query(schema).await?.check()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use surrealdb::RecordId;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct User {
        id: RecordId,
        name: String,
        email: String,
        oauth_provider: String,
        oauth_id: String,
    }

    #[tokio::test]
    async fn init_test_db_succeeds() {
        let db = init_test_db().await.expect("init_test_db should succeed");

        // スキーマが適用されていることを確認
        let result = db.query("INFO FOR DB").await;
        assert!(result.is_ok(), "INFO FOR DB should succeed after schema init");
    }

    #[tokio::test]
    async fn create_and_query_user() {
        let db = init_test_db().await.expect("init_test_db should succeed");

        // ユーザーを作成
        let _: Option<User> = db
            .create("users")
            .content(serde_json::json!({
                "name": "Test User",
                "email": "test@example.com",
                "oauth_provider": "github",
                "oauth_id": "12345",
            }))
            .await
            .expect("user creation should succeed");

        // ユーザーを取得
        let users: Vec<User> = db
            .select("users")
            .await
            .expect("user select should succeed");

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "Test User");
        assert_eq!(users[0].email, "test@example.com");
        assert_eq!(users[0].oauth_provider, "github");
        assert_eq!(users[0].oauth_id, "12345");
    }
}
