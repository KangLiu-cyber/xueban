//! 驱动适配器：Axum REST API（/api/v1）。
//!
//! 与 docs/architecture.md §8.1 对齐：20 个端点全部落在此处。身份一律
//! 由 token 解析（`require_auth` 中间件注入 `AuthUser`），不接受客户端
//! 声明 user_id；错误映射 NotFound→404 / Conflict→409 / Invalid→400 /
//! Storage→500。限流：注册登录按 IP（30/min），其余按 token（300/min），
//! 固定窗口（见 middleware.rs）。
//!
//! Logout 为公开路由：吊销后的 token 已无法通过鉴权中间件，但注销必须
//! 幂等可用，故该处理器直接读 Authorization 头并调用服务。
//!
//! 六边形组装（P1-10）：本适配器只依赖 application 用例与 domain 端口，
//! 仓储具体实现由 bootstrap 实例化并注入；服务句柄为
//! `Arc<dyn Trait + Send + Sync>` trait 对象，不反向依赖 adapter-postgres。

pub mod agent;
pub mod auth;
pub mod error;
pub mod middleware;
pub mod paper;
pub mod quiz;
pub mod space;
pub mod wrong;

use std::sync::Arc;

use application::agent::AgentService;
use application::auth::AuthService;
use application::paper::PaperService;
use application::quiz::QuizService;
use application::space::SpaceService;
use application::wrong::WrongService;
use axum::Router;
use axum::middleware as axum_mw;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use domain::ports::{
    AnnotationRepository, CredentialIssuer, EventStore, ItemRepository, PaperRepository,
    PasswordHasher, QuestionRepository, QuizRecordRepository, TokenRepository, UserRepository,
    WorkspaceRepository, WrongItemRepository,
};

use middleware::RateLimiter;

pub type PgAuthService = AuthService<
    dyn UserRepository + Send + Sync,
    dyn TokenRepository + Send + Sync,
    dyn PasswordHasher + Send + Sync,
    dyn CredentialIssuer + Send + Sync,
>;

pub type PgSpaceService = SpaceService<
    dyn WorkspaceRepository + Send + Sync,
    dyn ItemRepository + Send + Sync,
    dyn AnnotationRepository + Send + Sync,
    dyn EventStore + Send + Sync,
>;

pub type PgQuizService = QuizService<
    dyn WorkspaceRepository + Send + Sync,
    dyn ItemRepository + Send + Sync,
    dyn QuestionRepository + Send + Sync,
    dyn QuizRecordRepository + Send + Sync,
    dyn WrongItemRepository + Send + Sync,
    dyn EventStore + Send + Sync,
>;

pub type PgWrongService =
    WrongService<dyn WrongItemRepository + Send + Sync, dyn QuestionRepository + Send + Sync>;

pub type PgPaperService = PaperService<
    dyn WorkspaceRepository + Send + Sync,
    dyn ItemRepository + Send + Sync,
    dyn QuestionRepository + Send + Sync,
    dyn PaperRepository + Send + Sync,
    dyn WrongItemRepository + Send + Sync,
    dyn EventStore + Send + Sync,
>;

pub type PgAgentService = AgentService<
    dyn WorkspaceRepository + Send + Sync,
    dyn ItemRepository + Send + Sync,
    dyn QuestionRepository + Send + Sync,
    dyn EventStore + Send + Sync,
>;

/// 应用状态：全部用例服务（bootstrap 预组装注入）+ MCP 网关地址 + 限流器。
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
    /// 接收 bootstrap 注入的预组装服务；mcp_endpoint 为 MCP 网关对外地址，
    /// 经凭证端点随 token 下发给 Agent。
    pub fn new(
        auth: Arc<PgAuthService>,
        space: Arc<PgSpaceService>,
        quiz: Arc<PgQuizService>,
        wrong: Arc<PgWrongService>,
        paper: Arc<PgPaperService>,
        agent: Arc<PgAgentService>,
        mcp_endpoint: String,
    ) -> Self {
        Self {
            auth,
            space,
            quiz,
            wrong,
            paper,
            agent,
            mcp_endpoint,
            limiter: Arc::new(RateLimiter::default()),
        }
    }
}

/// 探针端点（§12 可观测）：容器 healthcheck / 反向代理探活用，无鉴权、无副作用。
async fn healthz() -> impl IntoResponse {
    axum::Json(serde_json::json!({ "status": "ok" }))
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
        .route("/workspaces/{id}", put(space::update_workspace))
        .route("/workspaces/{id}/tree", get(space::tree))
        .route(
            "/items/{id}",
            get(space::read_item).delete(space::delete_item),
        )
        .route("/items/{id}/annotations", post(space::add_annotation))
        .route(
            "/annotations/{id}",
            put(space::edit_annotation).delete(space::delete_annotation),
        )
        .route("/quiz/questions", get(quiz::draw))
        .route("/quiz/answer", post(quiz::answer))
        .route("/wrong", get(wrong::list))
        .route("/wrong/stats", get(wrong::stats))
        .route("/wrong/{id}/master", post(wrong::mark_mastered))
        .route("/wrong/{id}/unmaster", post(wrong::unmark_mastered))
        .route("/papers", post(paper::assemble))
        .route("/papers/{id}", get(paper::read))
        .route("/papers/{id}/submit", post(paper::submit))
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

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
        ]);

    Router::new()
        .nest("/api/v1", public.merge(protected))
        .route("/healthz", axum::routing::get(healthz))
        .layer(cors)
        .with_state(state)
}
