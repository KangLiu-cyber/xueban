//! 集成测试公共设施：DATABASE_URL 未设置时静默跳过（cargo test 无需数据库保持绿）。
//! 每次运行先应用迁移，保证测试在干净结构上执行。

use adapter_postgres::{PgUserRepository, RandomCredentialIssuer};
use chrono::Utc;
use domain::identity::User;
use domain::ports::{CredentialIssuer, UserRepository};

/// 连接池；环境变量缺失或连接失败 → None，调用方直接 return 跳过。
pub async fn pool() -> Option<sqlx::PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return None;
    };
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()
}

/// 应用迁移；schema 建不出来是测试环境的硬错误。
pub async fn setup(pool: &sqlx::PgPool) {
    adapter_postgres::migrate(pool).await.expect("迁移失败");
}

/// 唯一性时间戳：并发测试进程下账户名不冲突。
pub fn stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("时钟在 1970 之前")
        .as_nanos() as u64
}

/// 插入一个测试用户（凭证随机、哈希占位），返回其 id。
pub async fn insert_user(repo: &PgUserRepository, tag: &str) -> i64 {
    let user = User {
        id: 0,
        account: format!("{tag}_{}", stamp()),
        password_hash: RandomCredentialIssuer.issue(),
        nickname: Some(format!("{tag}-昵称")),
        created_at: Utc::now(),
    };
    repo.insert(&user).await.expect("插入用户失败")
}
