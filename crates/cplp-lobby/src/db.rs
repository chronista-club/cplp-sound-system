//! SurrealDB 接続管理
//!
//! REQ-LOBBY-001: ロビーサーバーのデータ永続化

pub use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// SurrealDB クライアント型エイリアス
pub type Db = Surreal<Any>;

/// 文字列 "table:id" を RecordId に変換する
pub fn to_record_id(s: &str) -> anyhow::Result<RecordId> {
    let (table, id) = s
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid record ID format (expected 'table:id'): {}", s))?;
    Ok(RecordId::from((table, id)))
}

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

    #[test]
    fn to_record_id_valid() {
        let rid = to_record_id("users:abc123").unwrap();
        assert_eq!(rid, RecordId::from(("users", "abc123")));
    }

    #[test]
    fn to_record_id_no_colon() {
        let result = to_record_id("no-colon");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid record ID format")
        );
    }

    #[test]
    fn to_record_id_empty() {
        assert!(to_record_id("").is_err());
    }

    #[test]
    fn to_record_id_multiple_colons() {
        // split_once は最初の ':' で分割 → table="groups", id="complex:id"
        let rid = to_record_id("groups:complex:id").unwrap();
        assert_eq!(rid, RecordId::from(("groups", "complex:id")));
    }

    #[tokio::test]
    async fn init_test_db_succeeds() {
        let db = init_test_db().await.expect("init_test_db should succeed");

        // スキーマが適用されていることを確認
        let result = db.query("INFO FOR DB").await;
        assert!(
            result.is_ok(),
            "INFO FOR DB should succeed after schema init"
        );
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
