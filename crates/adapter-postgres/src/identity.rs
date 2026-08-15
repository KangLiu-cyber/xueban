//! 身份域输出端口实现：users / tokens 两张表。
//!
//! 密码以 argon2id 哈希落库（OWASP 参数），登录凭证为随机 32 字节
//! Base62 编码、usr_ 前缀；token 吊销是幂等更新，不删除行（审计保留）。

use argon2::password_hash::{
    PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng as ArgonOsRng,
};
use argon2::{Algorithm, Argon2, Params, Version};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::error::{Error, Result};
use domain::identity::{Token, TokenPurpose, User};
use domain::ports::{CredentialIssuer, PasswordHasher, TokenRepository, UserRepository};
use rand::RngCore;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::map_sqlx_error;

pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn insert(&self, user: &User) -> Result<i64> {
        let row = sqlx::query(
            "insert into users (account, password_hash, nickname, created_at)
             values ($1, $2, $3, $4) returning id",
        )
        .bind(&user.account)
        .bind(&user.password_hash)
        .bind(&user.nickname)
        .bind(user.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn find_by_account(&self, account: &str) -> Result<Option<User>> {
        sqlx::query(
            "select id, account, password_hash, nickname, created_at
             from users where account = $1",
        )
        .bind(account)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| user_from_row(&row))
        .transpose()
    }

    async fn find_by_id(&self, id: i64) -> Result<Option<User>> {
        sqlx::query(
            "select id, account, password_hash, nickname, created_at
             from users where id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| user_from_row(&row))
        .transpose()
    }
}

fn user_from_row(row: &PgRow) -> Result<User> {
    Ok(User {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        account: row
            .try_get::<String, _>("account")
            .map_err(map_sqlx_error)?,
        password_hash: row
            .try_get::<String, _>("password_hash")
            .map_err(map_sqlx_error)?,
        nickname: row
            .try_get::<Option<String>, _>("nickname")
            .map_err(map_sqlx_error)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
    })
}

pub struct PgTokenRepository {
    pool: PgPool,
}

impl PgTokenRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TokenRepository for PgTokenRepository {
    async fn insert(&self, token: &Token) -> Result<i64> {
        let row = sqlx::query(
            "insert into tokens (user_id, token, purpose, revoked_at)
             values ($1, $2, $3, $4) returning id",
        )
        .bind(token.user_id)
        .bind(&token.token)
        .bind(token.purpose.as_str())
        .bind(token.revoked_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn find_by_token(&self, token: &str) -> Result<Option<Token>> {
        sqlx::query(
            "select id, user_id, token, purpose, revoked_at
             from tokens where token = $1",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| token_from_row(&row))
        .transpose()
    }

    async fn find_active_by_user_purpose(
        &self,
        user_id: i64,
        purpose: TokenPurpose,
    ) -> Result<Option<Token>> {
        sqlx::query(
            "select id, user_id, token, purpose, revoked_at
             from tokens where user_id = $1 and purpose = $2 and revoked_at is null
             order by id desc limit 1",
        )
        .bind(user_id)
        .bind(purpose.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| token_from_row(&row))
        .transpose()
    }

    async fn revoke(&self, token: &str, now: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            "update tokens set revoked_at = $2
             where token = $1 and revoked_at is null",
        )
        .bind(token)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn revoke_by_user_purpose(
        &self,
        user_id: i64,
        purpose: TokenPurpose,
        now: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "update tokens set revoked_at = $2
             where user_id = $1 and purpose = $3 and revoked_at is null",
        )
        .bind(user_id)
        .bind(now)
        .bind(purpose.as_str())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}

fn token_from_row(row: &PgRow) -> Result<Token> {
    let purpose: String = row.try_get("purpose").map_err(map_sqlx_error)?;
    Ok(Token {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        user_id: row.try_get::<i64, _>("user_id").map_err(map_sqlx_error)?,
        token: row.try_get::<String, _>("token").map_err(map_sqlx_error)?,
        purpose: purpose_from_str(&purpose)?,
        revoked_at: row
            .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
            .map_err(map_sqlx_error)?,
    })
}

fn purpose_from_str(s: &str) -> Result<TokenPurpose> {
    match s {
        "client" => Ok(TokenPurpose::Client),
        "agent" => Ok(TokenPurpose::Agent),
        other => Err(Error::Storage(format!("未知 token 用途: {other}"))),
    }
}

/// argon2id 哈希实现：OWASP 推荐参数（m=19_456 KiB, t=2, p=1）。
pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    fn hash(&self, plain: &str) -> Result<String> {
        let salt = SaltString::generate(&mut ArgonOsRng);
        let params = Params::new(19_456, 2, 1, None)
            .map_err(|e| Error::Storage(format!("argon2 参数错误: {e}")))?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        argon2::password_hash::PasswordHasher::hash_password(&argon, plain.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| Error::Storage(format!("密码哈希失败: {e}")))
    }

    fn verify(&self, plain: &str, hash: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok()
    }
}

/// Base62 编码：data-encoding 只支持 2 的幂字母表，故手写重复除法实现。
fn base62_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut n = bytes.to_vec(); // 大端无符号整数
    let mut out = Vec::with_capacity(43);
    while n.iter().any(|&b| b != 0) {
        let mut rem: u32 = 0;
        for b in &mut n {
            let cur = rem * 256 + u32::from(*b);
            *b = (cur / 62) as u8;
            rem = cur % 62;
        }
        out.push(ALPHABET[rem as usize]);
    }
    if out.is_empty() {
        out.push(ALPHABET[0]);
    }
    out.reverse();
    String::from_utf8(out).expect("base62 字母表为 ASCII")
}

/// 随机凭证签发：32 随机字节 → Base62 编码，usr_ 前缀（安全章节约定）。
pub struct RandomCredentialIssuer;

impl CredentialIssuer for RandomCredentialIssuer {
    fn issue(&self) -> String {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        format!("usr_{}", base62_encode(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuer_produces_unique_prefixed_credentials() {
        let issuer = RandomCredentialIssuer;
        let a = issuer.issue();
        let b = issuer.issue();
        assert_ne!(a, b);
        assert!(a.starts_with("usr_"));
        // 5 字符前缀 + 32 字节 Base62（43 字符）。
        assert!(a.len() >= 36);
        assert!(a[4..].chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn base62_encodes_known_vectors() {
        assert_eq!(base62_encode(&[0]), "0");
        assert_eq!(base62_encode(&[61]), "z");
        assert_eq!(base62_encode(&[62]), "10");
        assert_eq!(base62_encode(&[255]), "47");
        // 前导零字节不携带信息：编码保持注入性。
        assert_eq!(base62_encode(&[0, 255]), base62_encode(&[255]));
        // 32 字节全零 → 值 0 → "0"。
        assert_eq!(base62_encode(&[0u8; 32]), "0");
    }

    #[test]
    fn argon2_round_trip() {
        let hasher = Argon2PasswordHasher;
        let hash = hasher.hash("secret-123").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(hasher.verify("secret-123", &hash));
        assert!(!hasher.verify("wrong-pass", &hash));
        // 脏输入不 panic。
        assert!(!hasher.verify("secret-123", "garbage"));
        assert!(!hasher.verify("secret-123", ""));
    }

    #[test]
    fn purpose_parses_known_and_rejects_unknown() {
        assert_eq!(purpose_from_str("client").unwrap(), TokenPurpose::Client);
        assert_eq!(purpose_from_str("agent").unwrap(), TokenPurpose::Agent);
        assert!(purpose_from_str("admin").is_err());
    }
}
