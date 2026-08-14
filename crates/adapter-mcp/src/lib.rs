//! MCP 网关适配器（docs/architecture.md §8.2）。
//!
//! 经 rmcp streamable-http 暴露 10 个工具（见 tools.rs）；/mcp 全路由挂
//! `require_auth` 中间件：先按 token 限流（60/min）再校验 token，通过后
//! 注入 `AuthUser`，工具从请求扩展取回用户上下文。应用服务在此独立组装
//! （与 adapter-http 各自持有仓储组合，互不依赖）。

pub mod auth;
pub mod tools;

use std::sync::Arc;

use adapter_postgres::{
    Argon2PasswordHasher, PgAnnotationRepository, PgEventStore, PgItemRepository,
    PgQuestionRepository, PgTokenRepository, PgUserRepository, PgWorkspaceRepository,
    RandomCredentialIssuer,
};
use application::agent::AgentService;
use application::auth::AuthService;
use application::space::SpaceService;
use axum::Router;
use axum::middleware as axum_mw;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::auth::RateLimiter;
use crate::tools::McpService;

pub type PgAuthService =
    AuthService<PgUserRepository, PgTokenRepository, Argon2PasswordHasher, RandomCredentialIssuer>;

pub type PgSpaceService =
    SpaceService<PgWorkspaceRepository, PgItemRepository, PgAnnotationRepository, PgEventStore>;

pub type PgAgentService =
    AgentService<PgWorkspaceRepository, PgItemRepository, PgQuestionRepository, PgEventStore>;

/// MCP 网关状态：鉴权服务 + 空间/Agent 服务 + 限流器（bootstrap 组装注入）。
#[derive(Clone)]
pub struct McpState {
    pub auth: Arc<PgAuthService>,
    pub space: Arc<PgSpaceService>,
    pub agent: Arc<PgAgentService>,
    pub limiter: Arc<RateLimiter>,
}

impl McpState {
    /// 以同一连接池构造全部服务。
    pub fn new(pool: sqlx::PgPool) -> Self {
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
            agent: Arc::new(AgentService::new(
                Arc::new(PgWorkspaceRepository::new(pool.clone())),
                Arc::new(PgItemRepository::new(pool.clone())),
                Arc::new(PgQuestionRepository::new(pool.clone())),
                Arc::new(PgEventStore::new(pool)),
            )),
            limiter: Arc::new(RateLimiter::default()),
        }
    }
}

/// 组装 MCP 网关路由：/mcp 先限流再鉴权（防认证风暴），通过后进入
/// streamable-http 服务；每次请求由 service_factory 新建 McpService。
pub fn router(state: McpState) -> Router {
    Router::new()
        .route_service("/mcp", streamable_http_service(state.clone()))
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
