//! 启动入口：读取配置 → 连接 PostgreSQL → 跑迁移 → 组装 REST 与 MCP 路由一并 serve。
//!
//! 环境变量：
//! - `DATABASE_URL`：PostgreSQL 连接串（必填）；
//! - `BIND_ADDR`：监听地址，默认 `127.0.0.1:8080`；
//! - `MCP_ENDPOINT`：MCP 网关对外地址（随 Agent 凭证下发），默认 `http://<BIND_ADDR>/mcp`；
//! - `SKILLS_DIR`：内置 Skill 目录，默认 `skills`（相对工作目录，一个子文件夹一个 skill）；
//! - `ATTACHMENTS_DIR`：附件二进制根目录，默认 `attachments`（相对工作目录，布局
//!   `{user_id}/{uuid}`，见 §8.1 附件端点）。
//!
//! 前端 web 产物不由本进程静态托管：部署侧由 nginx 直接 serve 前端产物并反向代理
//! `/api/v1`、`/mcp`、`/healthz` 到本进程（见 deploy/nginx.conf、docs/architecture.md §11）。
//!
//! 六边形组装（P1-10）：仓储具体实现（adapter-postgres）在此实例化，
//! 以 `Arc<dyn Trait + Send + Sync>` 注入用例服务（application），再注入
//! 两个驱动适配器——驱动适配器不接触任何仓储实现。

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use adapter_postgres::{
    Argon2PasswordHasher, FsAttachmentStorage, PgAnnotationRepository, PgAttachmentRepository,
    PgEventStore, PgItemRepository, PgPaperRepository, PgQuestionRepository,
    PgQuizRecordRepository, PgSkillRepository, PgTokenRepository, PgUserRepository,
    PgWorkspaceRepository, PgWrongItemRepository, RandomCredentialIssuer,
};
use application::agent::AgentService;
use application::attachments::AttachmentService;
use application::auth::AuthService;
use application::paper::PaperService;
use application::quiz::QuizService;
use application::space::SpaceService;
use application::training::TrainingService;
use application::wrong::WrongService;
use axum::Router;
use domain::ports::{
    AnnotationRepository, AttachmentRepository, AttachmentStorage, CredentialIssuer, EventStore,
    ItemRepository, PaperRepository, PasswordHasher, QuestionRepository, QuizRecordRepository,
    SkillRepository, TokenRepository, UserRepository, WorkspaceRepository, WrongItemRepository,
};
use domain::skill::{Skill, parse_skill_file};
use sqlx::postgres::PgPoolOptions;
use tracing::info;

/// 加载系统内置 Skill 目录：`skills/` 下一个子文件夹一个 skill，
/// 文件夹内取 `skill.md`（frontmatter name/description + 正文脚本，见 domain::skill）；
/// 用文件夹直接编辑更新，无需打包。按名排序；目录缺失快速失败，解析错误带文件名。
fn load_skills(dir: &Path) -> Result<Vec<Skill>, Box<dyn std::error::Error>> {
    if !dir.is_dir() {
        return Err(format!(
            "Skill 目录 `{}` 不存在：在仓库根创建 `skills/` 文件夹（一个子文件夹一个 skill，\
             内含 skill.md），或用 SKILLS_DIR 环境变量指定",
            dir.display()
        )
        .into());
    }
    let mut skills = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let stem = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let content = std::fs::read_to_string(path.join("skill.md")).map_err(
            |e| -> Box<dyn std::error::Error> {
                format!("Skill 文件夹 `{}` 缺少 skill.md：{e}", path.display()).into()
            },
        )?;
        skills.push(parse_skill_file(&stem, &content).map_err(
            |e| -> Box<dyn std::error::Error> {
                format!(
                    "Skill 文件 `{}` 解析失败：{e}",
                    path.join("skill.md").display()
                )
                .into()
            },
        )?);
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_url = std::env::var("DATABASE_URL").expect("环境变量 DATABASE_URL 未设置");
    let bind: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()
        .expect("BIND_ADDR 不是合法的地址:端口");
    let mcp_endpoint =
        std::env::var("MCP_ENDPOINT").unwrap_or_else(|_| format!("http://{bind}/mcp"));

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&db_url)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    info!(%bind, %mcp_endpoint, "服务启动完成");

    // ---- 组装：仓储（adapter-postgres）→ 用例服务（application）→ 注入驱动适配器 ----
    let users: Arc<dyn UserRepository + Send + Sync> =
        Arc::new(PgUserRepository::new(pool.clone()));
    let tokens: Arc<dyn TokenRepository + Send + Sync> =
        Arc::new(PgTokenRepository::new(pool.clone()));
    let hasher: Arc<dyn PasswordHasher + Send + Sync> = Arc::new(Argon2PasswordHasher);
    let issuer: Arc<dyn CredentialIssuer + Send + Sync> = Arc::new(RandomCredentialIssuer);
    let workspaces: Arc<dyn WorkspaceRepository + Send + Sync> =
        Arc::new(PgWorkspaceRepository::new(pool.clone()));
    let items: Arc<dyn ItemRepository + Send + Sync> =
        Arc::new(PgItemRepository::new(pool.clone()));
    let annotations: Arc<dyn AnnotationRepository + Send + Sync> =
        Arc::new(PgAnnotationRepository::new(pool.clone()));
    let questions: Arc<dyn QuestionRepository + Send + Sync> =
        Arc::new(PgQuestionRepository::new(pool.clone()));
    let quiz_records: Arc<dyn QuizRecordRepository + Send + Sync> =
        Arc::new(PgQuizRecordRepository::new(pool.clone()));
    let wrong_items: Arc<dyn WrongItemRepository + Send + Sync> =
        Arc::new(PgWrongItemRepository::new(pool.clone()));
    let papers: Arc<dyn PaperRepository + Send + Sync> =
        Arc::new(PgPaperRepository::new(pool.clone()));
    let skills_repo: Arc<dyn SkillRepository + Send + Sync> =
        Arc::new(PgSkillRepository::new(pool.clone()));
    let events: Arc<dyn EventStore + Send + Sync> = Arc::new(PgEventStore::new(pool.clone()));

    // 附件：宿主磁盘存二进制（{ATTACHMENTS_DIR}/{user_id}/{uuid}），元数据走仓储。
    // 启动即建根目录，目录缺失/不可写提前暴露而非等到上传时才报错。
    let attachments_dir =
        std::env::var("ATTACHMENTS_DIR").unwrap_or_else(|_| "attachments".to_owned());
    std::fs::create_dir_all(&attachments_dir)?;
    info!(dir = %attachments_dir, "附件存储目录就绪");
    let attachment_repo: Arc<dyn AttachmentRepository + Send + Sync> =
        Arc::new(PgAttachmentRepository::new(pool.clone()));
    let attachment_storage: Arc<dyn AttachmentStorage + Send + Sync> =
        Arc::new(FsAttachmentStorage::new(attachments_dir));
    let attachments = Arc::new(AttachmentService::new(
        items.clone(),
        attachment_repo.clone(),
        attachment_storage,
    ));

    // 内置 Skill 目录：启动时从 `skills/` 文件夹加载（开发者维护的静态资产）。
    let skills_dir = std::env::var("SKILLS_DIR").unwrap_or_else(|_| "skills".to_owned());
    let skills = load_skills(Path::new(&skills_dir))?;
    info!(count = skills.len(), dir = %skills_dir, "内置 Skill 目录加载完成");

    let auth = Arc::new(AuthService::new(users, tokens, hasher, issuer));
    let space = Arc::new(SpaceService::new(
        workspaces.clone(),
        items.clone(),
        annotations,
        events.clone(),
    ));
    let quiz = Arc::new(QuizService::new(
        workspaces.clone(),
        items.clone(),
        questions.clone(),
        quiz_records,
        wrong_items.clone(),
        events.clone(),
        attachment_repo.clone(),
    ));
    let wrong = Arc::new(WrongService::new(wrong_items.clone(), questions.clone()));
    let paper = Arc::new(PaperService::new(
        workspaces.clone(),
        items.clone(),
        questions.clone(),
        papers,
        wrong_items,
        events.clone(),
    ));
    let training = Arc::new(TrainingService::new(events.clone()));
    let agent = Arc::new(AgentService::new(
        workspaces,
        items,
        questions,
        events,
        skills_repo,
        skills,
    ));

    let http_state = adapter_http::AppState::new(
        auth.clone(),
        space.clone(),
        quiz,
        wrong,
        paper,
        agent.clone(),
        attachments.clone(),
        training,
        mcp_endpoint,
    );
    let mcp_state = adapter_mcp::McpState::new(auth, space, agent, attachments);

    // 前端 web 产物由部署侧 nginx 托管并反代，本进程只挂 REST 与 MCP 路由
    // （见 deploy/nginx.conf、docs/architecture.md §11）。
    let app: Router = adapter_http::router(http_state).merge(adapter_mcp::router(mcp_state));

    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
