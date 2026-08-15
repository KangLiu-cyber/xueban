//! MCP 网关适配器（docs/architecture.md §8.2）。
//!
//! 经 rmcp streamable-http 暴露 12 个工具（见 tools.rs）；/mcp 全路由挂
//! `require_auth` 中间件：先按 token 限流（60/min）再校验 token，通过后
//! 注入 `AuthUser`，工具从请求扩展取回用户上下文。用例服务由 bootstrap
//! 预组装注入（P1-10：仓储实例化上移 bootstrap，本适配器只依赖
//! application 用例与 domain 端口）。

pub mod auth;
pub mod tools;

use std::sync::Arc;

use application::agent::AgentService;
use application::attachments::AttachmentService;
use application::auth::AuthService;
use application::space::SpaceService;
use axum::Router;
use axum::middleware as axum_mw;
use domain::ports::{
    AnnotationRepository, AttachmentRepository, AttachmentStorage, CredentialIssuer, EventStore,
    ItemRepository, PasswordHasher, QuestionRepository, SkillRepository, TokenRepository,
    UserRepository, WorkspaceRepository,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::auth::RateLimiter;
use crate::tools::McpService;

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

pub type PgAgentService = AgentService<
    dyn WorkspaceRepository + Send + Sync,
    dyn ItemRepository + Send + Sync,
    dyn QuestionRepository + Send + Sync,
    dyn EventStore + Send + Sync,
    dyn SkillRepository + Send + Sync,
>;

pub type PgAttachmentService = AttachmentService<
    dyn ItemRepository + Send + Sync,
    dyn AttachmentRepository + Send + Sync,
    dyn AttachmentStorage + Send + Sync,
>;

/// MCP 网关状态：鉴权服务 + 空间/Agent/附件服务（bootstrap 预组装注入）+ 限流器。
#[derive(Clone)]
pub struct McpState {
    pub auth: Arc<PgAuthService>,
    pub space: Arc<PgSpaceService>,
    pub agent: Arc<PgAgentService>,
    pub attachments: Arc<PgAttachmentService>,
    pub limiter: Arc<RateLimiter>,
}

impl McpState {
    /// 接收 bootstrap 注入的预组装服务。
    pub fn new(
        auth: Arc<PgAuthService>,
        space: Arc<PgSpaceService>,
        agent: Arc<PgAgentService>,
        attachments: Arc<PgAttachmentService>,
    ) -> Self {
        Self {
            auth,
            space,
            agent,
            attachments,
            limiter: Arc::new(RateLimiter::default()),
        }
    }
}

/// 组装 MCP 网关路由：/mcp 先限流再鉴权（防认证风暴），通过后进入
/// streamable-http 服务；每次请求由 service_factory 新建 McpService。
/// 请求体放宽到 16MB：upload_attachment 的 base64 图（10MB 上限 → 约 13.3MB
/// base64）必须能过缓冲，默认 2MB 会拒。
pub fn router(state: McpState) -> Router {
    Router::new()
        .route_service("/mcp", streamable_http_service(state.clone()))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .route_layer(axum_mw::from_fn_with_state(state, auth::require_auth))
}

/// 构建 streamable-http 服务：会话状态在服务内维护，工具实例按请求创建。
fn streamable_http_service(state: McpState) -> StreamableHttpService<McpService> {
    StreamableHttpService::new(
        move || Ok(McpService::new(Arc::new(state.clone()))),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}
