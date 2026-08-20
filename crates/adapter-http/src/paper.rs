//! 组卷模考端点：组卷 / 读卷 / 交卷（§8.1）。
//!
//! 组卷数量 1..=200（安全章节上界）；交卷走 `PaperBundle`/`PaperResult`，
//! chosen 与抽题同线格式。

use application::paper::{PaperAnswer, PaperBundle, SubmitOutcome};
use axum::Json;
use axum::extract::{Path, State};
use domain::error::Error;
use domain::practice::PaperConfig;
use serde::Deserialize;

use crate::AppState;
use crate::error::{ApiError, Json as JsonBody};
use crate::middleware::AuthUser;

#[derive(Debug, Deserialize)]
pub struct AssembleRequest {
    pub workspace_id: i64,
    pub name: Option<String>,
    pub config: PaperConfig,
}

/// POST /api/v1/papers
pub async fn assemble(
    State(state): State<AppState>,
    auth: AuthUser,
    JsonBody(body): JsonBody<AssembleRequest>,
) -> Result<Json<PaperBundle>, ApiError> {
    if body.config.count == 0 || body.config.count > 200 {
        return Err(ApiError::from(Error::Invalid(
            "组卷数量需在 1..=200 之间".to_owned(),
        )));
    }
    Ok(Json(
        state
            .paper
            .assemble(auth.0.id, body.workspace_id, body.name, body.config)
            .await?,
    ))
}

/// GET /api/v1/papers/:id
pub async fn read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<PaperBundle>, ApiError> {
    Ok(Json(state.paper.read(auth.0.id, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct SubmitRequest {
    pub answers: Vec<PaperAnswer>,
    pub duration_secs: u32,
}

/// POST /api/v1/papers/:id/submit
pub async fn submit(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    JsonBody(body): JsonBody<SubmitRequest>,
) -> Result<Json<SubmitOutcome>, ApiError> {
    Ok(Json(
        state
            .paper
            .submit(auth.0.id, id, body.answers, body.duration_secs)
            .await?,
    ))
}
