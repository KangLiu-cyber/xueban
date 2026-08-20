//! 身份与认证上下文：User、Token、Password 值对象。
//!
//! 密码不存明文：领域层只做强度校验（`Password::validate`），argon2id 哈希
//! 经 `PasswordHasher` 端口注入（见 `ports`）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// 登录 token 的用途：客户端或 Agent。同一用户两者各持一个，可单独吊销。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenPurpose {
    Client,
    Agent,
}

impl TokenPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenPurpose::Client => "client",
            TokenPurpose::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub account: String,
    pub password_hash: String,
    pub nickname: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    pub purpose: TokenPurpose,
    pub revoked_at: Option<DateTime<Utc>>,
    /// 最近一次鉴权通过的时间（记录登录/活跃时间）。
    pub last_used_at: Option<DateTime<Utc>>,
    /// 过期时间：None 表示永不过期（agent token / 存量 token）。
    pub expires_at: Option<DateTime<Utc>>,
}

impl Token {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    /// 是否已过期（仅对设置了 expires_at 的 client token 生效）。
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|e| e <= now)
    }

    /// 滑动续期：刷新活跃时间并把过期时间顺延一个 TTL。仅对带过期时间的
    /// client token 有意义；无过期时间的 token 调用后仍保持 None。
    pub fn touch(&mut self, now: DateTime<Utc>, ttl: chrono::Duration) {
        self.last_used_at = Some(now);
        if self.expires_at.is_some() {
            self.expires_at = Some(now + ttl);
        }
    }

    pub fn revoke(&mut self, now: DateTime<Utc>) {
        if self.revoked_at.is_none() {
            self.revoked_at = Some(now);
        }
    }
}

/// 客户端 token 滑动有效期：连续 30 天不活跃则失效，需重新登录；
/// 期间每次鉴权通过都会顺延（「频繁使用即不退出」）。
pub const CLIENT_TOKEN_TTL: chrono::Duration = chrono::Duration::days(30);

/// 已通过强度校验的明文密码（不持久化；持久化的是哈希）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Password(String);

const PASSWORD_MIN_LEN: usize = 8;
const PASSWORD_MAX_LEN: usize = 128;

impl Password {
    /// 校验明文强度并包装。argon2id 对任意长度输入均安全，但长度下限防弱口令。
    pub fn validate(plain: &str) -> Result<Password> {
        let len = plain.chars().count();
        if !(PASSWORD_MIN_LEN..=PASSWORD_MAX_LEN).contains(&len) {
            return Err(Error::Invalid(format!(
                "密码长度需在 {PASSWORD_MIN_LEN}~{PASSWORD_MAX_LEN} 个字符之间"
            )));
        }
        Ok(Password(plain.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_rejects_short() {
        assert!(matches!(
            Password::validate("short"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn password_rejects_empty() {
        assert!(matches!(Password::validate(""), Err(Error::Invalid(_))));
    }

    #[test]
    fn password_rejects_overlong() {
        let long = "x".repeat(PASSWORD_MAX_LEN + 1);
        assert!(matches!(Password::validate(&long), Err(Error::Invalid(_))));
    }

    #[test]
    fn password_accepts_valid_range() {
        assert!(Password::validate("12345678").is_ok());
        let boundary = "x".repeat(PASSWORD_MAX_LEN);
        assert!(Password::validate(&boundary).is_ok());
    }

    #[test]
    fn password_count_is_chars_not_bytes() {
        // 8 个中文字符：UTF-8 下 24 字节，但按字符计数应为 8，合法。
        assert!(Password::validate("一二三四五六七八").is_ok());
    }

    #[test]
    fn token_revoke_is_idempotent() {
        let mut t = Token {
            id: 1,
            user_id: 1,
            token: "usr_test".into(),
            purpose: TokenPurpose::Client,
            revoked_at: None,
            last_used_at: None,
            expires_at: None,
        };
        assert!(!t.is_revoked());
        let now = Utc::now();
        t.revoke(now);
        assert!(t.is_revoked());
        t.revoke(now); // 重复吊销不改变结果
        assert!(t.is_revoked());
    }

    #[test]
    fn token_touch_slides_expiry_but_keeps_none_unchanged() {
        let now = Utc::now();
        // 无过期时间（agent/存量 token）：touch 后仍不过期。
        let mut perpetual = Token {
            id: 1,
            user_id: 1,
            token: "usr_agent".into(),
            purpose: TokenPurpose::Agent,
            revoked_at: None,
            last_used_at: None,
            expires_at: None,
        };
        perpetual.touch(now, CLIENT_TOKEN_TTL);
        assert!(!perpetual.is_expired(now));
        assert_eq!(perpetual.expires_at, None);
        assert_eq!(perpetual.last_used_at, Some(now));

        // 带过期时间（client token）：每次 touch 顺延一个 TTL。
        let mut client = Token {
            id: 2,
            user_id: 1,
            token: "usr_client".into(),
            purpose: TokenPurpose::Client,
            revoked_at: None,
            last_used_at: None,
            expires_at: Some(now + CLIENT_TOKEN_TTL),
        };
        let later = now + chrono::Duration::days(10);
        client.touch(later, CLIENT_TOKEN_TTL);
        assert_eq!(client.expires_at, Some(later + CLIENT_TOKEN_TTL));
        assert!(!client.is_expired(later));
        assert!(client.is_expired(later + CLIENT_TOKEN_TTL + chrono::Duration::seconds(1)));
    }
}
