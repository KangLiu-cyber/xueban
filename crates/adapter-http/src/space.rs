//! 学习空间端点：工作空间 / 目录树 / 笔记与批注（§8.1）。
//!
//! 笔记内容由 Agent 经 MCP 生成，客户端只读；批注由客户端添加，
//! 作者固定为 User。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use domain::space::{Annotation, AnnotationAuthor, Item, ItemNode, Workspace};
use serde::Deserialize;

use crate::AppState;
use crate::error::{ApiError, Json as JsonBody};
use crate::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct WorkspaceInput {
    pub name: String,
    pub exam_goal: String,
    pub exam_date: Option<NaiveDate>,
}

/// GET /api/v1/workspaces
pub async fn list_workspaces(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Workspace>>, ApiError> {
    Ok(Json(state.space.list_workspaces(auth.0.id).await?))
}

/// POST /api/v1/workspaces
pub async fn create_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    JsonBody(body): JsonBody<WorkspaceInput>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(
        state
            .space
            .create_workspace(auth.0.id, body.name, body.exam_goal, body.exam_date)
            .await?,
    ))
}

/// PUT /api/v1/workspaces/:id
pub async fn update_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    JsonBody(body): JsonBody<WorkspaceInput>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(
        state
            .space
            .update_workspace(auth.0.id, id, body.name, body.exam_goal, body.exam_date)
            .await?,
    ))
}

/// GET /api/v1/workspaces/:id/tree
pub async fn tree(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Vec<ItemNode>>, ApiError> {
    Ok(Json(state.space.tree(auth.0.id, id).await?))
}

/// 笔记详情：正文 + 全部批注（§8.1 GET /items/:id）。
#[derive(Debug, serde::Serialize)]
pub struct ItemBundle {
    pub item: Item,
    pub annotations: Vec<Annotation>,
}

/// GET /api/v1/items/:id
pub async fn read_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<ItemBundle>, ApiError> {
    let item = state.space.read_item(auth.0.id, id).await?;
    let annotations = state.space.list_annotations(auth.0.id, id).await?;
    Ok(Json(ItemBundle { item, annotations }))
}

#[derive(Debug, Deserialize)]
pub struct AnnotationInput {
    pub anchor: String,
    pub text: String,
}

/// POST /api/v1/items/:id/annotations
pub async fn add_annotation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    JsonBody(body): JsonBody<AnnotationInput>,
) -> Result<Json<Annotation>, ApiError> {
    Ok(Json(
        state
            .space
            .annotate(
                auth.0.id,
                id,
                body.anchor,
                body.text,
                AnnotationAuthor::User,
            )
            .await?,
    ))
}

/// DELETE /api/v1/annotations/:id —— 批注是独立资源，id 走自己的路由。
pub async fn delete_annotation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Response {
    match state.space.delete_annotation(auth.0.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

/// DELETE /api/v1/items/:id —— 删除目录/笔记，级联子树/批注/归属题目。
pub async fn delete_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Response {
    match state.space.delete_item(auth.0.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
