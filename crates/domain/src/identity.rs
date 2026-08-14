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
}

impl Token {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    pub fn revoke(&mut self, now: DateTime<Utc>) {
        if self.revoked_at.is_none() {
            self.revoked_at = Some(now);
        }
    }
}

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
        };
        assert!(!t.is_revoked());
        let now = Utc::now();
        t.revoke(now);
        assert!(t.is_revoked());
        t.revoke(now); // 重复吊销不改变结果
        assert!(t.is_revoked());
    }
}
