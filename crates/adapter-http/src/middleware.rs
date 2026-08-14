//! 鉴权与限流中间件。
//!
//! `require_auth` 解析 Bearer token → 校验 → 把 `AuthUser` 注入请求扩展，
//! 处理器经 `AuthUser` 提取器拿到 user_id（数据隔离第一道防线）。
//! 限流为固定窗口：注册/登录按 IP（30/min），其余按 token（300/min），
//! 无 token 时退化为 IP。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::identity::User;
use serde_json::json;

use crate::AppState;

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

/// 已鉴权用户：`require_auth` 注入扩展，处理器经此提取 user_id。
#[derive(Clone)]
pub struct AuthUser(pub User);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(crate::error::unauthorized)
    }
}

/// 鉴权中间件：token 无效/已吊销 → 401（NotFound 与 Invalid 在此同义）。
pub async fn require_auth(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let Some(token) = bearer_token(req.headers()) else {
        return crate::error::unauthorized();
    };
    match state.auth.authenticate(&token).await {
        Ok(user) => {
            req.extensions_mut().insert(AuthUser(user));
            next.run(req).await
        }
        Err(e) => match e {
            domain::error::Error::Invalid(msg) | domain::error::Error::NotFound(msg) => (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({ "error": msg })),
            )
                .into_response(),
            other => crate::error::ApiError::from(other).into_response(),
        },
    }
}

const REGISTER_LIMIT: u32 = 30;
const API_LIMIT: u32 = 300;
const WINDOW: Duration = Duration::from_secs(60);

/// 注册/登录限流：按 IP。
pub async fn rate_limit_by_ip(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let ip = client_ip(&req);
    if state
        .limiter
        .check(&format!("ip:{ip}"), REGISTER_LIMIT, WINDOW)
    {
        next.run(req).await
    } else {
        crate::error::too_many_requests()
    }
}

/// 其余端点限流：按 token；无 token 时退化为 IP。
pub async fn rate_limit_by_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let key = match bearer_token(req.headers()) {
        Some(token) => format!("token:{token}"),
        None => format!("ip:{}", client_ip(&req)),
    };
    if state.limiter.check(&key, API_LIMIT, WINDOW) {
        next.run(req).await
    } else {
        crate::error::too_many_requests()
    }
}

fn client_ip(req: &Request) -> String {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.to_string())
        .unwrap_or_else(|| "unknown".to_owned())
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
