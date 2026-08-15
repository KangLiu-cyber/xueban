//! 错误映射与请求体提取。
//!
//! 领域错误 → HTTP 状态码：NotFound→404 / Conflict→409 / Invalid→400 /
//! Storage→500；Storage 细节不外泄，统一返回"服务内部错误"。
//! `Json<T>` 提取器把 axum 的 JSON 解析失败映射为 400（默认是 422/415）。
//! `ApiError` 是本地新类型（孤儿规则：不能为外部类型实现外来 trait）。

use axum::extract::FromRequest;
use axum::extract::Request;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::error::Error;
use std::error::Error as StdError;

/// 领域错误的 HTTP 适配：处理器返回 `Result<T, ApiError>`。
#[derive(Debug)]
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self.0 {
            Error::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            Error::Conflict(msg) => (StatusCode::CONFLICT, msg),
            Error::Invalid(msg) => (StatusCode::BAD_REQUEST, msg),
            Error::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "服务内部错误".to_owned()),
        };
        error_response(status, msg)
    }
}

/// 统一错误响应体：`{"error": "<msg>"}`。
pub fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (
        status,
        axum::Json(serde_json::json!({ "error": msg.into() })),
    )
        .into_response()
}

pub fn unauthorized() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "未登录")
}

pub fn too_many_requests() -> Response {
    error_response(StatusCode::TOO_MANY_REQUESTS, "请求过于频繁，请稍后再试")
}

/// 请求体提取器：解析失败统一映射为 400。
#[derive(Debug)]
pub struct Json<T>(pub T);

impl<S, T> FromRequest<S> for Json<T>
where
    axum::Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Json(v)),
            Err(e) => Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("请求体格式错误: {}", e.body_text()),
            )),
        }
    }
}

/// 原始字节请求体提取器：附件上传用（raw bytes，非 multipart）。
/// 上传路由由 RequestBodyLimitLayer（tower-http limit）包住 Body，超过
/// 上限在缓冲阶段以 LengthLimitError 终止，这里沿错误链识别并映射为
/// 413；其余缓冲失败映射为 400。
///
/// 不走 `Bytes::from_request`：其拒绝值（BytesRejection）的负载字段是
/// pub(crate)，外部无法解构取出限制错误；改为手动 `collect` 整个 Body。
#[derive(Debug)]
pub struct RawBody(pub Vec<u8>);

impl<S> FromRequest<S> for RawBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let body = axum::body::Body::from_request(req, state)
            .await
            .map_err(|_| error_response(StatusCode::BAD_REQUEST, "请求体读取失败"))?;
        match http_body_util::BodyExt::collect(body).await {
            Ok(collected) => Ok(RawBody(collected.to_bytes().to_vec())),
            Err(e) => {
                // axum::Error 的 source() 即内部 BoxError，从这里沿链找 LengthLimitError。
                let mut source: Option<&(dyn StdError + 'static)> = e.source();
                let too_large = loop {
                    match source {
                        Some(s) if s.is::<http_body_util::LengthLimitError>() => break true,
                        Some(s) => source = s.source(),
                        None => break false,
                    }
                };
                if too_large {
                    Err(error_response(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "请求体超过大小上限",
                    ))
                } else {
                    Err(error_response(StatusCode::BAD_REQUEST, "请求体读取失败"))
                }
            }
        }
    }
}
