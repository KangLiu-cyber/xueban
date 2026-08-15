//! 附件端点：上传 / 读取 / 删除（§8.1）。
//!
//! 上传为 raw bytes（非 multipart，避免 axum multipart 限制坑），Content-Type
//! 白名单与魔数嗅探在用例层完成；读取返回存储 mime + `X-Content-Type-Options:
//! nosniff`——`<img>` 带不了 Authorization header，前端 fetch→blob 渲染。

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::{ApiError, RawBody};
use crate::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    /// 可选原始文件名（仅展示用）；缺省时按魔数 mime 给默认。
    pub name: Option<String>,
}

/// POST /api/v1/items/:id/attachments —— raw bytes 上传（≤10MB，魔数嗅探，仅 note）。
pub async fn upload(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(item_id): Path<i64>,
    Query(query): Query<UploadQuery>,
    RawBody(bytes): RawBody,
) -> Result<Response, ApiError> {
    let att = state
        .attachments
        .upload(auth.0.id, item_id, query.name.unwrap_or_default(), &bytes)
        .await?;
    Ok((StatusCode::CREATED, axum::Json(att)).into_response())
}

/// GET /api/v1/attachments/:id —— 返回存储 mime 的二进制 + nosniff。
pub async fn read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let (att, bytes) = state.attachments.read(auth.0.id, id).await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, att.mime),
            (header::CONTENT_LENGTH, att.size_bytes.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        bytes,
    )
        .into_response())
}

/// DELETE /api/v1/attachments/:id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Response {
    match state.attachments.delete(auth.0.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
