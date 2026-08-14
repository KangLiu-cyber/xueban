//! 驱动适配器：Axum REST API（/api/v1）。
//!
//! 与 docs/architecture.md §8.1 对齐：19 个端点全部落在此处。身份一律
//! 由 token 解析（`require_auth` 中间件注入 `AuthUser`），不接受客户端
//! 声明 user_id；错误映射 NotFound→404 / Conflict→409 / Invalid→400 /
//! Storage→500。限流：注册登录按 IP（30/min），其余按 token（300/min），
//! 固定窗口（见 middleware.rs）。
//!
//! Logout 为公开路由：吊销后的 token 已无法通过鉴权中间件，但注销必须
//! 幂等可用，故该处理器直接读 Authorization 头并调用服务。

pub mod agent;
pub mod auth;
pub mod error;
pub mod middleware;
pub mod paper;
pub mod quiz;
pub mod space;
pub mod wrong;

use std::sync::Arc;

use adapter_postgres::{
    Argon2PasswordHasher, PgAnnotationRepository, PgEventStore, PgItemRepository,
    PgPaperRepository, PgQuestionRepository, PgQuizRecordRepository, PgTokenRepository,
    PgUserRepository, PgWorkspaceRepository, PgWrongItemRepository, RandomCredentialIssuer,
};
use application::agent::AgentService;
use application::auth::AuthService;
use application::paper::PaperService;
use application::quiz::QuizService;
use application::space::SpaceService;
use application::wrong::WrongService;
use axum::Router;
use axum::middleware as axum_mw;
use axum::routing::{delete, get, post, put};

use middleware::RateLimiter;

pub type PgAuthService =
    AuthService<PgUserRepository, PgTokenRepository, Argon2PasswordHasher, RandomCredentialIssuer>;

pub type PgSpaceService =
    SpaceService<PgWorkspaceRepository, PgItemRepository, PgAnnotationRepository, PgEventStore>;

pub type PgQuizService = QuizService<
    PgWorkspaceRepository,
    PgItemRepository,
    PgQuestionRepository,
    PgQuizRecordRepository,
    PgWrongItemRepository,
    PgEventStore,
>;

pub type PgWrongService = WrongService<PgWrongItemRepository, PgQuestionRepository>;

pub type PgPaperService = PaperService<
    PgWorkspaceRepository,
    PgItemRepository,
    PgQuestionRepository,
    PgPaperRepository,
    PgWrongItemRepository,
    PgEventStore,
>;

pub type PgAgentService =
    AgentService<PgWorkspaceRepository, PgItemRepository, PgQuestionRepository, PgEventStore>;

/// 应用状态：全部具体服务 + MCP 网关地址 + 限流器（bootstrap 组装注入）。
#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<PgAuthService>,
    pub space: Arc<PgSpaceService>,
    pub quiz: Arc<PgQuizService>,
    pub wrong: Arc<PgWrongService>,
    pub paper: Arc<PgPaperService>,
    pub agent: Arc<PgAgentService>,
    pub mcp_endpoint: String,
    pub limiter: Arc<RateLimiter>,
}

impl AppState {
    /// 以同一连接池构造全部服务；mcp_endpoint 为 MCP 网关对外地址，
    /// 经凭证端点随 token 下发给 Agent。
    pub fn new(pool: sqlx::PgPool, mcp_endpoint: String) -> Self {
        Self {
            auth: Arc::new(AuthService::new(
                Arc::new(PgUserRepository::new(pool.clone())),
                Arc::new(PgTokenRepository::new(pool.clone())),
                Arc::new(Argon2PasswordHasher),
                Arc::new(RandomCredentialIssuer),
            )),
            space: Arc::new(SpaceService::new(
                Arc::new(PgWorkspaceRepository::new(pool.clone())),
                Arc::new(PgItemRepository::new(pool.clone())),
                Arc::new(PgAnnotationRepository::new(pool.clone())),
                Arc::new(PgEventStore::new(pool.clone())),
            )),
            quiz: Arc::new(QuizService::new(
                Arc::new(PgWorkspaceRepository::new(pool.clone())),
                Arc::new(PgItemRepository::new(pool.clone())),
                Arc::new(PgQuestionRepository::new(pool.clone())),
                Arc::new(PgQuizRecordRepository::new(pool.clone())),
                Arc::new(PgWrongItemRepository::new(pool.clone())),
                Arc::new(PgEventStore::new(pool.clone())),
            )),
            wrong: Arc::new(WrongService::new(
                Arc::new(PgWrongItemRepository::new(pool.clone())),
                Arc::new(PgQuestionRepository::new(pool.clone())),
            )),
            paper: Arc::new(PaperService::new(
                Arc::new(PgWorkspaceRepository::new(pool.clone())),
                Arc::new(PgItemRepository::new(pool.clone())),
                Arc::new(PgQuestionRepository::new(pool.clone())),
                Arc::new(PgPaperRepository::new(pool.clone())),
                Arc::new(PgWrongItemRepository::new(pool.clone())),
                Arc::new(PgEventStore::new(pool.clone())),
            )),
            agent: Arc::new(AgentService::new(
                Arc::new(PgWorkspaceRepository::new(pool.clone())),
                Arc::new(PgItemRepository::new(pool.clone())),
                Arc::new(PgQuestionRepository::new(pool.clone())),
                Arc::new(PgEventStore::new(pool)),
            )),
            mcp_endpoint,
            limiter: Arc::new(RateLimiter::default()),
        }
    }
}

/// 组装 /api/v1 路由。公开路由（注册/登录/注销）与鉴权路由分开挂中间件，
/// `.layer` 后调用者在外层：鉴权路由先限流（防认证风暴）再鉴权。
pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            middleware::rate_limit_by_ip,
        ));

    let protected = Router::new()
        .route(
            "/workspaces",
            get(space::list_workspaces).post(space::create_workspace),
        )
        .route("/workspaces/:id", put(space::update_workspace))
        .route("/workspaces/:id/tree", get(space::tree))
        .route("/items/:id", get(space::read_item))
        .route("/items/:id/annotations", post(space::add_annotation))
        .route("/annotations/:id", delete(space::delete_annotation))
        .route("/quiz/questions", get(quiz::draw))
        .route("/quiz/answer", post(quiz::answer))
        .route("/wrong", get(wrong::list))
        .route("/wrong/:id/master", post(wrong::mark_mastered))
        .route("/papers", post(paper::assemble))
        .route("/papers/:id", get(paper::read))
        .route("/papers/:id/submit", post(paper::submit))
        .route("/agent/credential", get(agent::credential))
        .route("/agent/credential/rotate", post(agent::rotate_credential))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            middleware::require_auth,
        ))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            middleware::rate_limit_by_token,
        ));

    Router::new()
        .nest("/api/v1", public.merge(protected))
        .with_state(state)
}
