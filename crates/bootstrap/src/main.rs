//! 启动入口：读取配置 → 连接 PostgreSQL → 跑迁移 → 组装 REST 与 MCP 路由一并 serve。
//!
//! 环境变量：
//! - `DATABASE_URL`：PostgreSQL 连接串（必填）；
//! - `BIND_ADDR`：监听地址，默认 `127.0.0.1:8080`；
//! - `MCP_ENDPOINT`：MCP 网关对外地址（随 Agent 凭证下发），默认 `http://<BIND_ADDR>/mcp`。

use std::net::SocketAddr;

use axum::Router;
use sqlx::postgres::PgPoolOptions;
use tracing::info;

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

    let http_state = adapter_http::AppState::new(pool.clone(), mcp_endpoint);
    let mcp_state = adapter_mcp::McpState::new(pool);
    let app: Router = adapter_http::router(http_state).merge(adapter_mcp::router(mcp_state));

    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
