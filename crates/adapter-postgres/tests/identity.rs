//! 身份仓储集成测试（env-gated）：users / tokens 落库、查询与吊销生命周期。

mod common;

use adapter_postgres::{Argon2PasswordHasher, PgTokenRepository, PgUserRepository};
use chrono::Utc;
use common::{insert_user, pool, setup, stamp};
use domain::identity::{Token, TokenPurpose, User};
use domain::ports::{PasswordHasher, TokenRepository, UserRepository};

#[tokio::test]
async fn user_insert_and_find_round_trip() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let repo = PgUserRepository::new(pool);
    let id = insert_user(&repo, "alice").await;
    let by_id = repo
        .find_by_id(id)
        .await
        .expect("按 id 查询失败")
        .expect("应按 id 找到");
    let account = by_id.account.clone();
    let by_account = repo
        .find_by_account(&account)
        .await
        .expect("按账号查询失败")
        .expect("应按账号找到");
    assert_eq!(by_account.id, id);
    assert_eq!(by_account.account, account);
    assert_eq!(by_account.nickname.as_deref(), Some("alice-昵称"));
    assert_eq!(by_account, by_id);
    assert!(
        repo.find_by_account("no_such_account")
            .await
            .expect("查询失败")
            .is_none()
    );
    assert!(repo.find_by_id(999_999).await.expect("查询失败").is_none());
}

#[tokio::test]
async fn user_insert_conflicts_on_duplicate_account() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let repo = PgUserRepository::new(pool);
    let account = format!("dup_{}", stamp());
    let first = User {
        id: 0,
        account: account.clone(),
        password_hash: "h".into(),
        nickname: None,
        created_at: Utc::now(),
    };
    repo.insert(&first).await.expect("首次插入失败");
    let again = User {
        id: 0,
        account,
        password_hash: "h".into(),
        nickname: None,
        created_at: Utc::now(),
    };
    assert!(matches!(
        repo.insert(&again).await,
        Err(domain::error::Error::Conflict(_))
    ));
}

#[tokio::test]
async fn token_lifecycle_revoke_and_purpose_revoke() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "token").await;
    let repo = PgTokenRepository::new(pool);
    let client = Token {
        id: 0,
        user_id,
        token: format!("usr_client_{}", stamp()),
        purpose: TokenPurpose::Client,
        revoked_at: None,
    };
    let agent = Token {
        id: 0,
        user_id,
        token: format!("usr_agent_{}", stamp()),
        purpose: TokenPurpose::Agent,
        revoked_at: None,
    };
    repo.insert(&client).await.expect("插入 client token 失败");
    repo.insert(&agent).await.expect("插入 agent token 失败");

    // 按凭证查回（含未吊销状态）。
    let found = repo
        .find_by_token(&client.token)
        .await
        .expect("按凭证查询失败")
        .expect("应按凭证找到");
    assert_eq!(found.user_id, user_id);
    assert!(!found.is_revoked());
    assert!(
        repo.find_by_token("usr_unknown")
            .await
            .expect("查询失败")
            .is_none()
    );

    // 吊销单个：client 吊销，agent 不受影响。
    let now = Utc::now();
    repo.revoke(&client.token, now).await.expect("吊销失败");
    let revoked = repo
        .find_by_token(&client.token)
        .await
        .expect("查询失败")
        .expect("应仍可查回");
    assert!(revoked.is_revoked());
    let agent_alive = repo
        .find_by_token(&agent.token)
        .await
        .expect("查询失败")
        .expect("agent token 应未吊销");
    assert!(!agent_alive.is_revoked());

    // 按用户+用途吊销：agent 全部失效，client 已吊销不重复生效。
    repo.revoke_by_user_purpose(user_id, TokenPurpose::Agent, now)
        .await
        .expect("按用途吊销失败");
    assert!(
        repo.find_by_token(&agent.token)
            .await
            .expect("查询失败")
            .expect("应查回")
            .is_revoked()
    );
    // 重复吊销幂等。
    repo.revoke_by_user_purpose(user_id, TokenPurpose::Agent, now)
        .await
        .expect("重复吊销失败");
    // 其他用户不受影响。
    let other = insert_user(&user_repo, "other").await;
    let foreign = Token {
        id: 0,
        user_id: other,
        token: format!("usr_other_{}", stamp()),
        purpose: TokenPurpose::Agent,
        revoked_at: None,
    };
    repo.insert(&foreign).await.expect("插入他人 token 失败");
    repo.revoke_by_user_purpose(user_id, TokenPurpose::Agent, now)
        .await
        .expect("吊销失败");
    assert!(
        !repo
            .find_by_token(&foreign.token)
            .await
            .expect("查询失败")
            .expect("应查回")
            .is_revoked()
    );
}

#[tokio::test]
async fn token_find_active_by_user_purpose_returns_latest_active_only() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "active").await;
    let repo = PgTokenRepository::new(pool);

    // 未签发时查不到。
    assert!(
        repo.find_active_by_user_purpose(user_id, TokenPurpose::Agent)
            .await
            .expect("查询失败")
            .is_none()
    );

    let agent1 = Token {
        id: 0,
        user_id,
        token: format!("usr_agent1_{}", stamp()),
        purpose: TokenPurpose::Agent,
        revoked_at: None,
    };
    let agent2 = Token {
        id: 0,
        user_id,
        token: format!("usr_agent2_{}", stamp()),
        purpose: TokenPurpose::Agent,
        revoked_at: None,
    };
    let client = Token {
        id: 0,
        user_id,
        token: format!("usr_client_{}", stamp()),
        purpose: TokenPurpose::Client,
        revoked_at: None,
    };
    repo.insert(&agent1).await.expect("插入失败");
    repo.insert(&agent2).await.expect("插入失败");
    repo.insert(&client).await.expect("插入失败");

    // 取最新一条（agent2），client 用途互不干扰。
    let found = repo
        .find_active_by_user_purpose(user_id, TokenPurpose::Agent)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_eq!(found.token, agent2.token);

    // 吊销最新后回退到上一代；全吊销后查不到。
    repo.revoke(&agent2.token, Utc::now())
        .await
        .expect("吊销失败");
    let prev = repo
        .find_active_by_user_purpose(user_id, TokenPurpose::Agent)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_eq!(prev.token, agent1.token);
    repo.revoke(&agent1.token, Utc::now())
        .await
        .expect("吊销失败");
    assert!(
        repo.find_active_by_user_purpose(user_id, TokenPurpose::Agent)
            .await
            .expect("查询失败")
            .is_none()
    );
    // client 用途仍可取到自己的现行凭证。
    assert!(
        repo.find_active_by_user_purpose(user_id, TokenPurpose::Client)
            .await
            .expect("查询失败")
            .is_some()
    );
}

#[tokio::test]
async fn argon2_hash_and_verify_against_db() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let hasher = Argon2PasswordHasher;
    let hash = hasher.hash("passw0rd").expect("哈希失败");
    let user = domain::identity::User {
        id: 0,
        account: format!("hashed_{}", stamp()),
        password_hash: hash.clone(),
        nickname: None,
        created_at: Utc::now(),
    };
    let repo = PgUserRepository::new(pool);
    repo.insert(&user).await.expect("插入用户失败");
    let stored = repo
        .find_by_account(&user.account)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_ne!(stored.password_hash, "passw0rd");
    assert!(hasher.verify("passw0rd", &stored.password_hash));
    assert!(!hasher.verify("wrong", &stored.password_hash));
}
