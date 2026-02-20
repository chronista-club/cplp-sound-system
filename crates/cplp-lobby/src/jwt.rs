//! JWT 発行・検証
//!
//! REQ-LOBBY-002: 認証トークン管理

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// JWT クレーム
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// ユーザーID（SurrealDB レコード ID）
    pub sub: String,
    /// 有効期限（UNIX タイムスタンプ）
    pub exp: u64,
    /// 発行日時（UNIX タイムスタンプ）
    pub iat: u64,
}

/// JWT を発行する
///
/// 有効期限は発行から7日間。
pub fn create_token(user_id: &str, secret: &str) -> anyhow::Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let claims = Claims {
        sub: user_id.to_string(),
        exp: now + 7 * 24 * 60 * 60, // 7日間
        iat: now,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// 指定した有効期限で JWT を発行する（テスト用）
#[cfg(test)]
fn create_token_with_exp(user_id: &str, secret: &str, exp: u64) -> anyhow::Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        iat: now,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// JWT を検証してクレームを返す
pub fn verify_token(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-jwt";

    #[test]
    fn test_create_and_verify_token() {
        let user_id = "users:abc123";
        let token = create_token(user_id, TEST_SECRET).expect("token creation should succeed");

        let claims = verify_token(&token, TEST_SECRET).expect("token verification should succeed");
        assert_eq!(claims.sub, user_id);
        assert!(claims.exp > claims.iat);
        assert_eq!(claims.exp - claims.iat, 7 * 24 * 60 * 60);
    }

    #[test]
    fn test_expired_token() {
        let user_id = "users:expired";
        // 過去の有効期限を設定
        let token = create_token_with_exp(user_id, TEST_SECRET, 1_000_000)
            .expect("token creation should succeed");

        let result = verify_token(&token, TEST_SECRET);
        assert!(result.is_err(), "expired token should fail verification");
    }

    #[test]
    fn test_invalid_token() {
        let result = verify_token("not-a-valid-jwt-token", TEST_SECRET);
        assert!(result.is_err(), "invalid token should fail verification");
    }

    #[test]
    fn test_wrong_secret() {
        let user_id = "users:wrong";
        let token =
            create_token(user_id, TEST_SECRET).expect("token creation should succeed");

        let result = verify_token(&token, "wrong-secret");
        assert!(
            result.is_err(),
            "token verified with wrong secret should fail"
        );
    }
}
