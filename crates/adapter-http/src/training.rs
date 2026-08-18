//! 训练打卡端点（体育领域包）：打卡 / 历史列表（§8.1）。

use application::training::{CheckinInput, CheckinRecord};
use axum::Json;
use axum::extract::{Query, State};
use domain::error::Error;
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiError;
use crate::middleware::AuthUser;

/// GET /api/v1/training/checkins — limit 默认 20，上限 100。
#[derive(Debug, Deserialize)]
pub struct ListCheckinsQuery {
    pub limit: Option<u32>,
}

/// POST /api/v1/training/checkin
pub async fn checkin(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(input): Json<CheckinInput>,
) -> Result<Json<CheckinRecord>, ApiError> {
    Ok(Json(state.training.checkin(auth.0.id, None, input).await?))
}

/// GET /api/v1/training/checkins
pub async fn checkins(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListCheckinsQuery>,
) -> Result<Json<Vec<CheckinRecord>>, ApiError> {
    let limit = q.limit.unwrap_or(20);
    if limit == 0 || limit > 100 {
        return Err(ApiError::from(Error::Invalid(
            "limit 需在 1..=100 之间".to_owned(),
        )));
    }
    Ok(Json(state.training.list(auth.0.id, limit).await?))
}
