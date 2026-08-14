//! MCP 网关鉴权与限流中间件（架构文档 §8.2 强制规则）。
//!
//! 连接建立时校验 Bearer token → 把 `AuthUser` 注入请求扩展；rmcp 将
//! http Parts（含扩展）拷入 JSON-RPC 消息扩展，工具经 `Extension<Parts>`
//! 取回用户上下文——工具入参中不存在用户身份字段（§10 数据隔离）。
//! 限流为固定窗口，按 token 60 次/分钟（§8.2 强制规则）。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::identity::User;
use serde_json::json;

use crate::McpState;

/// 固定窗口限流器：key → (窗口起点, 计数)。窗口到点即重置。
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}

struct Bucket {
    window_start: Instant,
    count: u32,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }
}

impl RateLimiter {
    /// 返回 false 表示超限（本次请求应被拒绝）。
    pub fn check(&self, key: &str, limit: u32, window: Duration) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        let bucket = buckets.entry(key.to_owned()).or_insert_with(|| Bucket {
            window_start: Instant::now(),
            count: 0,
        });
        if bucket.window_start.elapsed() >= window {
            bucket.window_start = Instant::now();
            bucket.count = 0;
        }
        if bucket.count >= limit {
            return false;
        }
        bucket.count += 1;
        true
    }
}

/// 从请求头解析 Bearer token（`Authorization: Bearer <token>`）。
pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_owned())
}

/// 已鉴权用户：`require_auth` 注入请求扩展，rmcp 拷入消息扩展供工具提取。
#[derive(Clone)]
pub struct AuthUser(pub User);

const MCP_LIMIT: u32 = 60;
const WINDOW: Duration = Duration::from_secs(60);

/// MCP 网关鉴权中间件：先按 token 限流（无 token 退化为 IP），
/// 再校验 token（无效/已吊销 → 401），通过后注入 `AuthUser`。
pub async fn require_auth(State(state): State<McpState>, mut req: Request, next: Next) -> Response {
    let Some(token) = bearer_token(req.headers()) else {
        return error_response(StatusCode::UNAUTHORIZED, "未登录");
    };
    let key = match req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.to_string())
    {
        Some(ip) => format!("token:{token}:{ip}"),
        None => format!("token:{token}"),
    };
    if !state.limiter.check(&key, MCP_LIMIT, WINDOW) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "请求过于频繁，请稍后再试");
    }
    match state.auth.authenticate(&token).await {
        Ok(user) => {
            req.extensions_mut().insert(AuthUser(user));
            next.run(req).await
        }
        Err(e) => match e {
            domain::error::Error::Invalid(msg) | domain::error::Error::NotFound(msg) => {
                error_response(StatusCode::UNAUTHORIZED, &msg)
            }
            other => {
                tracing::error!(error = %other, "MCP 鉴权存储失败");
                error_response(StatusCode::INTERNAL_SERVER_ERROR, "服务内部错误")
            }
        },
    }
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_within_window_and_blocks_beyond() {
        let limiter = RateLimiter::default();
        let window = Duration::from_secs(60);
        assert!(limiter.check("k", 3, window));
        assert!(limiter.check("k", 3, window));
        assert!(limiter.check("k", 3, window));
        assert!(!limiter.check("k", 3, window));
        // 不同 key 互不影响。
        assert!(limiter.check("other", 3, window));
    }

    #[test]
    fn limiter_resets_after_window_elapses() {
        let limiter = RateLimiter::default();
        let window = Duration::from_millis(10);
        assert!(limiter.check("k", 1, window));
        assert!(!limiter.check("k", 1, window));
        std::thread::sleep(Duration::from_millis(50));
        // 窗口已过，计数重置。
        assert!(limiter.check("k", 1, window));
    }
}
