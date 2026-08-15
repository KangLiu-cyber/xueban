//! 身份与认证用例：Register / Login / Logout / Agent 凭证换发。
//!
//! user_id 一律来自 token 解析结果（`authenticate`），不接受客户端声明。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::identity::{Password, Token, TokenPurpose, User};
use domain::ports::{CredentialIssuer, PasswordHasher, TokenRepository, UserRepository};

pub struct AuthService<U, T, H, I>
where
    U: UserRepository + ?Sized,
    T: TokenRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    I: CredentialIssuer + ?Sized,
{
    users: Arc<U>,
    tokens: Arc<T>,
    hasher: Arc<H>,
    issuer: Arc<I>,
}

impl<U, T, H, I> AuthService<U, T, H, I>
where
    U: UserRepository + ?Sized,
    T: TokenRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    I: CredentialIssuer + ?Sized,
{
    pub fn new(users: Arc<U>, tokens: Arc<T>, hasher: Arc<H>, issuer: Arc<I>) -> Self {
        Self {
            users,
            tokens,
            hasher,
            issuer,
        }
    }

    /// Register：密码强度校验 → 账号查重 → 哈希入库 → 签发 client token。
    pub async fn register(
        &self,
        account: &str,
        password: &str,
        nickname: Option<&str>,
    ) -> Result<(User, Token)> {
        let password = Password::validate(password)?;
        if self.users.find_by_account(account).await?.is_some() {
            return Err(Error::Conflict("账号已注册".to_owned()));
        }
        let hash = self.hasher.hash(password.as_str())?;
        let user = User {
            id: 0,
            account: account.to_owned(),
            password_hash: hash,
            nickname: nickname.map(str::to_owned),
            created_at: Utc::now(),
        };
        let id = self.users.insert(&user).await?;
        let mut user = user;
        user.id = id;
        let token = self.issue_token(user.id, TokenPurpose::Client).await?;
        Ok((user, token))
    }

    /// Login：查账号 → 验密码 → 吊销旧 client token → 签发新 token。
    /// 账号不存在与密码错误返回同一错误文案，防账号枚举。
    pub async fn login(&self, account: &str, password: &str) -> Result<Token> {
        let Some(user) = self.users.find_by_account(account).await? else {
            return Err(Error::Invalid("账号或密码错误".to_owned()));
        };
        if !self.hasher.verify(password, &user.password_hash) {
            return Err(Error::Invalid("账号或密码错误".to_owned()));
        }
        // 同一用户仅保留一个 client token：登录即吊销旧的。
        self.tokens
            .revoke_by_user_purpose(user.id, TokenPurpose::Client, Utc::now())
            .await?;
        self.issue_token(user.id, TokenPurpose::Client).await
    }

    /// Logout：吊销当前 token。已吊销则幂等成功。
    pub async fn logout(&self, token: &str) -> Result<()> {
        let Some(mut t) = self.tokens.find_by_token(token).await? else {
            return Err(Error::NotFound("凭证不存在".to_owned()));
        };
        if !t.is_revoked() {
            let now = Utc::now();
            t.revoke(now);
            self.tokens.revoke(token, now).await?;
        }
        Ok(())
    }

    /// 鉴权辅助：解析 token、校验用途并返回用户。驱动适配器统一经此解析身份；
    /// 用途不符（agent token 调 REST、client token 调 MCP）按凭证无效拒绝。
    pub async fn authenticate(&self, token: &str, purpose: TokenPurpose) -> Result<User> {
        let t = self
            .tokens
            .find_by_token(token)
            .await?
            .ok_or_else(|| Error::NotFound("凭证无效".to_owned()))?;
        if t.is_revoked() {
            return Err(Error::Invalid("凭证已吊销".to_owned()));
        }
        if t.purpose != purpose {
            return Err(Error::Invalid("凭证无效".to_owned()));
        }
        self.users
            .find_by_id(t.user_id)
            .await?
            .ok_or_else(|| Error::NotFound("用户不存在".to_owned()))
    }

    /// 读取现行 Agent 凭证（GET /agent/credential 用，不换发）。
    pub async fn agent_credential(&self, user_id: i64) -> Result<Token> {
        self.tokens
            .find_active_by_user_purpose(user_id, TokenPurpose::Agent)
            .await?
            .ok_or_else(|| Error::NotFound("尚无 Agent 凭证，请先换发".to_owned()))
    }

    /// 换发 Agent 凭证：吊销旧 agent token 后签发新 token。
    pub async fn rotate_agent_token(&self, user_id: i64) -> Result<Token> {
        self.tokens
            .revoke_by_user_purpose(user_id, TokenPurpose::Agent, Utc::now())
            .await?;
        self.issue_token(user_id, TokenPurpose::Agent).await
    }

    async fn issue_token(&self, user_id: i64, purpose: TokenPurpose) -> Result<Token> {
        let token = Token {
            id: 0,
            user_id,
            token: self.issuer.issue(),
            purpose,
            revoked_at: None,
        };
        let id = self.tokens.insert(&token).await?;
        let mut token = token;
        token.id = id;
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryTokenRepository, InMemoryUserRepository, TestCredentialIssuer, TestPasswordHasher,
    };

    fn svc() -> AuthService<
        InMemoryUserRepository,
        InMemoryTokenRepository,
        TestPasswordHasher,
        TestCredentialIssuer,
    > {
        AuthService::new(
            Arc::new(InMemoryUserRepository::default()),
            Arc::new(InMemoryTokenRepository::default()),
            Arc::new(TestPasswordHasher),
            Arc::new(TestCredentialIssuer::default()),
        )
    }

    #[tokio::test]
    async fn register_returns_user_and_client_token() {
        let s = svc();
        let (user, token) = s
            .register("alice", "password1", Some("爱丽丝"))
            .await
            .unwrap();
        assert_eq!(user.account, "alice");
        assert_eq!(user.nickname.as_deref(), Some("爱丽丝"));
        assert_eq!(token.user_id, user.id);
        assert_eq!(token.purpose, TokenPurpose::Client);
        assert!(!token.is_revoked());
    }

    #[tokio::test]
    async fn register_rejects_duplicate_account() {
        let s = svc();
        s.register("alice", "password1", None).await.unwrap();
        assert!(matches!(
            s.register("alice", "password1", None).await,
            Err(Error::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn register_rejects_weak_password() {
        let s = svc();
        assert!(matches!(
            s.register("bob", "short", None).await,
            Err(Error::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn login_issues_token_and_revokes_old_client_token() {
        let s = svc();
        let (_, first) = s.register("alice", "password1", None).await.unwrap();
        let second = s.login("alice", "password1").await.unwrap();
        assert_ne!(first.token, second.token);
        // 旧 client token 已吊销，新 token 有效。
        assert!(
            s.authenticate(&first.token, TokenPurpose::Client)
                .await
                .is_err()
        );
        assert!(
            s.authenticate(&second.token, TokenPurpose::Client)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn login_rejects_bad_credentials() {
        let s = svc();
        s.register("alice", "password1", None).await.unwrap();
        // 密码错误与账号不存在返回同一错误。
        assert!(matches!(
            s.login("alice", "wrongpass").await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            s.login("nobody", "password1").await,
            Err(Error::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn logout_revokes_token_and_is_idempotent() {
        let s = svc();
        let (_, token) = s.register("alice", "password1", None).await.unwrap();
        s.logout(&token.token).await.unwrap();
        assert!(matches!(
            s.authenticate(&token.token, TokenPurpose::Client).await,
            Err(Error::Invalid(_))
        ));
        // 重复注销不报错。
        s.logout(&token.token).await.unwrap();
    }

    #[tokio::test]
    async fn authenticate_rejects_unknown_token() {
        let s = svc();
        assert!(matches!(
            s.authenticate("usr_nonexistent", TokenPurpose::Client)
                .await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn authenticate_enforces_token_purpose() {
        let s = svc();
        let (user, client_token) = s.register("alice", "password1", None).await.unwrap();
        let agent_token = s.rotate_agent_token(user.id).await.unwrap();
        // client token 不能通过 agent 用途鉴权，反之亦然。
        assert!(
            s.authenticate(&client_token.token, TokenPurpose::Client)
                .await
                .is_ok()
        );
        assert!(
            s.authenticate(&client_token.token, TokenPurpose::Agent)
                .await
                .is_err()
        );
        assert!(
            s.authenticate(&agent_token.token, TokenPurpose::Agent)
                .await
                .is_ok()
        );
        assert!(
            s.authenticate(&agent_token.token, TokenPurpose::Client)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn agent_credential_reads_active_without_rotate() {
        let s = svc();
        let (user, _) = s.register("alice", "password1", None).await.unwrap();
        // 未换发过时读不到。
        assert!(matches!(
            s.agent_credential(user.id).await,
            Err(Error::NotFound(_))
        ));
        let issued = s.rotate_agent_token(user.id).await.unwrap();
        let read = s.agent_credential(user.id).await.unwrap();
        assert_eq!(read.id, issued.id);
        assert_eq!(read.token, issued.token);
        // 再换发后读到的是新凭证。
        let second = s.rotate_agent_token(user.id).await.unwrap();
        assert_eq!(s.agent_credential(user.id).await.unwrap().id, second.id);
    }

    #[tokio::test]
    async fn rotate_agent_token_revokes_previous_agent_token() {
        let s = svc();
        let (user, _) = s.register("alice", "password1", None).await.unwrap();
        let first = s.rotate_agent_token(user.id).await.unwrap();
        assert_eq!(first.purpose, TokenPurpose::Agent);
        let second = s.rotate_agent_token(user.id).await.unwrap();
        assert_ne!(first.token, second.token);
        assert!(
            s.authenticate(&first.token, TokenPurpose::Agent)
                .await
                .is_err()
        );
        assert!(
            s.authenticate(&second.token, TokenPurpose::Agent)
                .await
                .is_ok()
        );
        // client token 不受影响。
        assert!(s.login("alice", "password1").await.is_ok());
    }
}
