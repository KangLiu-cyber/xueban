//! 身份端点：Register / Login / Logout（§8.1）。
//!
//! user_id 一律来自 token 解析，不接受客户端声明；用户响应体
//! （`UserDto`）剥离 password_hash。

use axum::Json;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use domain::identity::User;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::{ApiError, Json as JsonBody};
use crate::middleware::bearer_token;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub account: String,
    pub password: String,
    pub nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub account: String,
    pub password: String,
}

/// 用户响应体：不含 password_hash。
#[derive(Debug, Clone, Serialize)]
pub struct UserDto {
    pub id: i64,
    pub account: String,
    pub nickname: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserDto {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            account: u.account,
            nickname: u.nickname,
            created_at: u.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDto,
}

/// POST /api/v1/auth/register
pub async fn register(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<RegisterRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let (user, token) = state
        .auth
        .register(&body.account, &body.password, body.nickname.as_deref())
        .await?;
    Ok(Json(AuthResponse {
        token: token.token,
        user: user.into(),
    }))
}

/// POST /api/v1/auth/login
pub async fn login(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let token = state.auth.login(&body.account, &body.password).await?;
    let user = state.auth.authenticate(&token.token).await?;
    Ok(Json(AuthResponse {
        token: token.token,
        user: user.into(),
    }))
}

/// POST /api/v1/auth/logout —— 公开路由：吊销后的 token 已过不了鉴权
/// 中间件，但注销必须幂等可用，故直接读 Authorization 头。
pub async fn logout(State(state): State<AppState>, req: Request) -> Response {
    let Some(token) = bearer_token(req.headers()) else {
        return crate::error::unauthorized();
    };
    match state.auth.logout(&token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
