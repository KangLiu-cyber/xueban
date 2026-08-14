//! Agent 接入端点：凭证读取 / 换发（§8.1）。
//!
//! 换发即吊销旧 agent token 后签发新 token；端点随凭证下发 MCP 网关
//! 地址（bootstrap 从环境变量注入），Agent 无需额外配置。

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::AppState;
use crate::error::ApiError;
use crate::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct CredentialResponse {
    pub token: String,
    pub endpoint: String,
}

/// GET /api/v1/agent/credential —— 读现行凭证，不换发。
pub async fn credential(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<CredentialResponse>, ApiError> {
    let token = state.auth.agent_credential(auth.0.id).await?;
    Ok(Json(CredentialResponse {
        token: token.token,
        endpoint: state.mcp_endpoint.clone(),
    }))
}

/// POST /api/v1/agent/credential/rotate —— 吊销旧凭证并换发。
pub async fn rotate_credential(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<CredentialResponse>, ApiError> {
    let token = state.auth.rotate_agent_token(auth.0.id).await?;
    Ok(Json(CredentialResponse {
        token: token.token,
        endpoint: state.mcp_endpoint.clone(),
    }))
}
