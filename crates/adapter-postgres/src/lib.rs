//! 被驱动适配器：SQLx + PostgreSQL 实现领域输出端口。
//!
//! 与 docs/architecture.md §5.2 / §7 对齐：每个聚合一张表，仓储方法在 SQL 层
//! 强制 user_id / workspace 归属条件（隔离第二道防线）；事件只追加不修改。
//! 密码哈希 argon2id；凭证签发为随机 32 字节 Base62（usr_ 前缀）。

mod attachments;
mod codec;
mod events;
mod identity;
mod practice;
mod skill;
mod space;

pub use attachments::{FsAttachmentStorage, PgAttachmentRepository};
pub use events::PgEventStore;
pub use identity::{
    Argon2PasswordHasher, PgTokenRepository, PgUserRepository, RandomCredentialIssuer,
};
pub use practice::{
    PgPaperRepository, PgQuestionRepository, PgQuizRecordRepository, PgWrongItemRepository,
};
pub use skill::PgSkillRepository;
pub use space::{PgAnnotationRepository, PgItemRepository, PgWorkspaceRepository};

use domain::error::{Error, Result};

/// 统一 SQLx 错误 → 领域错误：外键冲突（如并发删 item）→ NotFound，
/// 唯一约束冲突 → Conflict，其余 → Storage。
pub fn map_sqlx_error(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db) = &e {
        if db.is_foreign_key_violation() {
            return Error::NotFound("内容不存在".to_owned());
        }
        if db.is_unique_violation() {
            return Error::Conflict("记录已存在".to_owned());
        }
    }
    Error::Storage(e.to_string())
}

/// 把 `migrations/` 下的迁移应用到连接池（bootstrap 启动时调用）。
/// 用运行时 `Migrator::new` 而非 `migrate!` 宏，避免引入 macros 特性
/// 与编译期数据库依赖；路径锚定到本 crate 的清单目录。
pub async fn migrate(pool: &sqlx::PgPool) -> Result<()> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let migrator = sqlx::migrate::Migrator::new(dir)
        .await
        .map_err(|e| Error::Storage(e.to_string()))?;
    migrator
        .run(pool)
        .await
        .map_err(|e| Error::Storage(e.to_string()))
}
