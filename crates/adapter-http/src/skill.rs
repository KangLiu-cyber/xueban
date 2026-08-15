//! 自定义 Skill 端点：清单 / 新建 / 删除（§8.1）。
//!
//! 用户保存自己的 skill（名称 + 介绍 + 脚本），随 bootstrap 能力下发
//! 合并进能力包，Agent 按名经 MCP get_skill 拉取完整内容。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::ApiError;
use crate::error::Json as JsonBody;
use crate::middleware::AuthUser;

#[derive(Debug, Serialize)]
pub struct SkillDto {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct SkillCreateRequest {
    pub name: String,
    pub description: String,
    pub script: Option<String>,
}

impl From<domain::skill::UserSkill> for SkillDto {
    fn from(s: domain::skill::UserSkill) -> Self {
        Self {
            id: s.id,
            name: s.name,
            description: s.description,
        }
    }
}

/// GET /api/v1/agent/skills —— 我的自定义 skill 清单。
pub async fn list_skills(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<SkillDto>>, ApiError> {
    let skills = state.agent.list_skills(auth.0.id).await?;
    Ok(Json(skills.into_iter().map(SkillDto::from).collect()))
}

/// POST /api/v1/agent/skills —— 新建自定义 skill（同用户重名 → 409）。
pub async fn create_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    JsonBody(body): JsonBody<SkillCreateRequest>,
) -> Result<Json<SkillDto>, ApiError> {
    let skill = state
        .agent
        .create_skill(auth.0.id, body.name, body.description, body.script)
        .await?;
    Ok(Json(SkillDto::from(skill)))
}

/// DELETE /api/v1/agent/skills/:id —— 删除自定义 skill（未命中 → 404）。
pub async fn delete_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Response {
    match state.agent.delete_skill(auth.0.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
