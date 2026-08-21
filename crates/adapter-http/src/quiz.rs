//! 刷题端点：抽题 / 作答（§8.1）。
//!
//! 抽题按 `workspace_id`（必填）+ `scope`（集节点 id，可选）+ `count`
//! （默认 10，1..=100）；作答的 chosen 走 §8.1 线格式（single→数字、
//! multi→索引数组、judge→布尔，domain 层 untagged 序列化）。

use application::quiz::{AnswerOutcome, QuestionBrief};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use domain::error::Error;
use domain::practice::Chosen;
use serde::Deserialize;

use crate::AppState;
use crate::error::{ApiError, Json as JsonBody};
use crate::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct DrawQuery {
    pub workspace_id: i64,
    pub scope: Option<i64>,
    pub count: Option<u32>,
}

/// GET /api/v1/quiz/questions
pub async fn draw(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<DrawQuery>,
) -> Result<Json<Vec<QuestionBrief>>, ApiError> {
    // count 缺省或为 0 表示返回范围内全部题目；1..=100 表示最多返回 N 题。
    let count = q.count.unwrap_or(0);
    if count > 100 {
        return Err(ApiError::from(Error::Invalid(
            "count 需在 0..=100 之间".to_owned(),
        )));
    }
    Ok(Json(
        state
            .quiz
            .draw(auth.0.id, q.workspace_id, q.scope, count)
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct AnswerRequest {
    pub question_id: i64,
    pub chosen: Chosen,
    pub scope: Option<String>,
}

/// POST /api/v1/quiz/answer
pub async fn answer(
    State(state): State<AppState>,
    auth: AuthUser,
    JsonBody(body): JsonBody<AnswerRequest>,
) -> Result<Json<AnswerOutcome>, ApiError> {
    Ok(Json(
        state
            .quiz
            .submit(auth.0.id, body.question_id, body.chosen, body.scope)
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct VideoAnswerRequest {
    pub question_id: i64,
    /// 训练视频附件 id 列表（附件已先经 /items/:id/attachments 上传，挂在题源笔记下）。
    pub attachment_ids: Vec<i64>,
    pub note: Option<String>,
}

/// POST /api/v1/quiz/video-answer —— 视频题作答：不判分，落 video_submit 事件供 AI 复盘。
pub async fn video_answer(
    State(state): State<AppState>,
    auth: AuthUser,
    JsonBody(body): JsonBody<VideoAnswerRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .quiz
        .submit_video(auth.0.id, body.question_id, body.attachment_ids, body.note)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
