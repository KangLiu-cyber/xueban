//! 错题本端点：列表 / 标记掌握（§8.1）。
//!
//! 列表直接返回 `WrongListItem`（错题记录 + 题目简述），客户端渲染
//! 题干无需再回查题目。

use application::wrong::WrongListItem;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::error::ApiError;
use crate::middleware::AuthUser;

/// GET /api/v1/wrong
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<WrongListItem>>, ApiError> {
    Ok(Json(state.wrong.list(auth.0.id).await?))
}

/// POST /api/v1/wrong/:id/master
pub async fn mark_mastered(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Response {
    match state.wrong.mark_mastered(auth.0.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
