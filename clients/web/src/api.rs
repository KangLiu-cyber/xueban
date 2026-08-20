//! /api/v1 类型化客户端。全部请求经 gloo-net 发出；401 触发会话失效回调。
//! 与 crates/adapter-http 各 handler 的 wire shape 一一对应（docs/requirements.md §API）。
//!
//! API 基址解析（`base_url`）：
//! 1. 构建期环境变量 `XUEBAN_API_BASE`（web/desktop 部署时注入域名地址）；
//! 2. 浏览器运行期同源兜底 `location.origin + /api/v1`（§11 nginx 同域反代）；
//! 3. 均不可用时退回本地开发地址。

use chrono::{DateTime, NaiveDate, Utc};
use gloo_net::http::{Method, Request};
use serde::{Deserialize, Serialize};

pub fn base_url() -> String {
    if let Some(u) = option_env!("XUEBAN_API_BASE") {
        return u.trim_end_matches('/').to_owned();
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(origin) = web_sys::window().and_then(|w| w.location().origin().ok()) {
            if origin.starts_with("http://") || origin.starts_with("https://") {
                return format!("{}/api/v1", origin.trim_end_matches('/'));
            }
        }
    }
    "http://127.0.0.1:8080/api/v1".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub account: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub account: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDto {
    pub id: i64,
    pub account: String,
    pub nickname: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDto,
}

/// GET /auth/me 会话恢复响应：用户 + 最近活跃/过期时间（记录登录时间）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeResponse {
    pub user: UserDto,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInput {
    pub name: String,
    pub exam_goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exam_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub exam_goal: String,
    pub exam_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Dir,
    Note,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Creator {
    Agent,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub workspace_id: i64,
    pub parent_id: Option<i64>,
    pub kind: ItemKind,
    pub name: String,
    pub content: Option<String>,
    pub created_by: Creator,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemNode {
    pub item: Item,
    pub children: Vec<ItemNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationAuthor {
    User,
    Ai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: i64,
    pub item_id: i64,
    pub user_id: i64,
    pub author: AnnotationAuthor,
    pub anchor: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationInput {
    pub anchor: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemBundle {
    pub item: Item,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    Single,
    Multi,
    Judge,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionBrief {
    pub id: i64,
    pub source_item_id: i64,
    pub qtype: QuestionType,
    pub stem: String,
    pub options: Vec<String>,
}

/// 用户作答。single/judge 为下标（judge 0=错误 1=正确），multi 为下标集合。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Chosen {
    Single(usize),
    Multi(BTreeSetUsize),
    Judge(bool),
}

/// 后端 BTreeSet<usize> 序列化为有序数组；untagged 枚举里不能直接用 BTreeSet
/// （会与 usize 分支歧义），用透明 newtype 兜住集合分支。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct BTreeSetUsize(pub Vec<usize>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawQuery {
    pub workspace_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRequest {
    pub question_id: i64,
    pub chosen: Chosen,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerOutcome {
    pub is_correct: bool,
    pub answer: Chosen,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrongItem {
    pub id: i64,
    pub user_id: i64,
    pub question_id: i64,
    pub times: u32,
    pub mastered: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrongListItem {
    pub wrong: WrongItem,
    pub question: QuestionBrief,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrongStats {
    pub total: u32,
    pub weekly_new: u32,
    pub mastered: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paper {
    pub id: i64,
    pub user_id: i64,
    pub workspace_id: i64,
    #[serde(default)]
    pub name: Option<String>,
    pub config: PaperConfig,
    pub result: Option<PaperResult>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_types: Option<Vec<QuestionType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_item_ids: Option<Vec<i64>>,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembleRequest {
    pub workspace_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub config: PaperConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperBundle {
    pub paper: Paper,
    pub questions: Vec<QuestionBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperAnswer {
    pub question_id: i64,
    pub chosen: Chosen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRequest {
    pub answers: Vec<PaperAnswer>,
    pub duration_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperResult {
    pub score: u32,
    pub correct: u32,
    pub total: u32,
    pub duration_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialResponse {
    pub token: String,
    pub endpoint: String,
}

/// 服务端错误体统一为 {"error": "..."}。
#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
}

pub enum ApiError {
    Http(u16, String),
    Network(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Http(code, msg) => write!(f, "{} {}", code, msg),
            ApiError::Network(msg) => write!(f, "网络错误：{}", msg),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

// 会话失效回调：登录态由外层注入，401 时置空。
thread_local! {
    static AUTH_TOKEN_OPT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static ON_UNAUTHORIZED: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

pub fn set_unauthorized_handler(f: impl Fn() + 'static) {
    ON_UNAUTHORIZED.with(|c| *c.borrow_mut() = Some(Box::new(f)));
}

fn fire_unauthorized() {
    ON_UNAUTHORIZED.with(|c| {
        if let Some(f) = c.borrow().as_ref() {
            f();
        }
    });
}

pub fn set_auth_token(token: Option<String>) {
    AUTH_TOKEN_OPT.with(|c| *c.borrow_mut() = token);
}

async fn parse_error(text: &str) -> ApiError {
    if let Ok(body) = serde_json::from_str::<ApiErrorBody>(text) {
        ApiError::Http(0, body.error)
    } else {
        ApiError::Http(0, text.to_string())
    }
}

async fn send(method: &str, path: &str, body: Option<&str>, with_token: bool) -> ApiResult<String> {
    let url = format!("{}{}", base_url(), path);
    let m = match method {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => Method::GET,
    };
    let mut req = match m {
        Method::GET => Request::get(&url),
        Method::POST => Request::post(&url),
        Method::PUT => Request::put(&url),
        Method::DELETE => Request::delete(&url),
        _ => Request::get(&url),
    }
    .header("Content-Type", "application/json");
    if with_token {
        let token = AUTH_TOKEN_OPT.with(|c| c.borrow().clone());
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {}", t));
        }
    }
    let req = if let Some(b) = body {
        req.body(b).map_err(|e| ApiError::Network(e.to_string()))?
    } else {
        Request::try_from(req).map_err(|e| ApiError::Network(e.to_string()))?
    };
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    if (200..300).contains(&status) {
        Ok(text)
    } else {
        if status == 401 {
            fire_unauthorized();
        }
        Err(parse_error(&text).await)
    }
}

async fn get_json<T: for<'de> Deserialize<'de>>(path: &str) -> ApiResult<T> {
    let text = send("GET", path, None, true).await?;
    serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("解析失败：{}", e)))
}

async fn post_json<T: for<'de> Deserialize<'de>>(path: &str, body: &str) -> ApiResult<T> {
    let text = send("POST", path, Some(body), true).await?;
    serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("解析失败：{}", e)))
}

async fn put_json<T: for<'de> Deserialize<'de>>(path: &str, body: &str) -> ApiResult<T> {
    let text = send("PUT", path, Some(body), true).await?;
    serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("解析失败：{}", e)))
}

// ---- 公开路由 ----

pub async fn register(req: &RegisterRequest) -> ApiResult<AuthResponse> {
    let text = send(
        "POST",
        "/auth/register",
        Some(&serde_json::to_string(req).unwrap_or_default()),
        false,
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("解析失败：{}", e)))
}

pub async fn login(req: &LoginRequest) -> ApiResult<AuthResponse> {
    let text = send(
        "POST",
        "/auth/login",
        Some(&serde_json::to_string(req).unwrap_or_default()),
        false,
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| ApiError::Network(format!("解析失败：{}", e)))
}

pub async fn logout() -> ApiResult<()> {
    send("POST", "/auth/logout", None, true).await.map(|_| ())
}

/// 会话恢复：校验本地 token 并拉取最新用户信息与活跃时间（无感登录）。
pub async fn me() -> ApiResult<MeResponse> {
    get_json("/auth/me").await
}

// ---- 空间 ----

pub async fn list_workspaces() -> ApiResult<Vec<Workspace>> {
    get_json("/workspaces").await
}

pub async fn create_workspace(input: &WorkspaceInput) -> ApiResult<Workspace> {
    post_json(
        "/workspaces",
        &serde_json::to_string(input).unwrap_or_default(),
    )
    .await
}

pub async fn update_workspace(id: i64, input: &WorkspaceInput) -> ApiResult<Workspace> {
    put_json(
        &format!("/workspaces/{}", id),
        &serde_json::to_string(input).unwrap_or_default(),
    )
    .await
}

pub async fn delete_workspace(id: i64) -> ApiResult<()> {
    send("DELETE", &format!("/workspaces/{}", id), None, true)
        .await
        .map(|_| ())
}

pub async fn tree(workspace_id: i64) -> ApiResult<Vec<ItemNode>> {
    get_json(&format!("/workspaces/{}/tree", workspace_id)).await
}

pub async fn item_bundle(item_id: i64) -> ApiResult<ItemBundle> {
    get_json(&format!("/items/{}", item_id)).await
}

pub async fn add_annotation(item_id: i64, input: &AnnotationInput) -> ApiResult<Annotation> {
    post_json(
        &format!("/items/{}/annotations", item_id),
        &serde_json::to_string(input).unwrap_or_default(),
    )
    .await
}

pub async fn delete_annotation(id: i64) -> ApiResult<()> {
    send("DELETE", &format!("/annotations/{}", id), None, true)
        .await
        .map(|_| ())
}

// ---- 刷题 ----

pub async fn draw(query: &DrawQuery) -> ApiResult<Vec<QuestionBrief>> {
    let mut qs = vec![format!("workspace_id={}", query.workspace_id)];
    if let Some(s) = query.scope {
        qs.push(format!("scope={}", s));
    }
    if let Some(c) = query.count {
        qs.push(format!("count={}", c));
    }
    get_json(&format!("/quiz/questions?{}", qs.join("&"))).await
}

pub async fn answer(req: &AnswerRequest) -> ApiResult<AnswerOutcome> {
    post_json(
        "/quiz/answer",
        &serde_json::to_string(req).unwrap_or_default(),
    )
    .await
}

// ---- 错题本 ----

pub async fn wrong_list() -> ApiResult<Vec<WrongListItem>> {
    get_json("/wrong").await
}

pub async fn wrong_stats() -> ApiResult<WrongStats> {
    get_json("/wrong/stats").await
}

pub async fn mark_mastered(wrong_item_id: i64) -> ApiResult<()> {
    send(
        "POST",
        &format!("/wrong/{}/master", wrong_item_id),
        None,
        true,
    )
    .await
    .map(|_| ())
}

// ---- 训练打卡（体育领域包） ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinInput {
    pub sport: String,
    pub activity: String,
    pub duration_minutes: u32,
    pub rating: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckinRecord {
    pub id: i64,
    pub workspace_id: Option<i64>,
    pub sport: String,
    pub activity: String,
    pub duration_minutes: u32,
    pub rating: u8,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn checkin(input: &CheckinInput) -> ApiResult<CheckinRecord> {
    post_json(
        "/training/checkin",
        &serde_json::to_string(input).unwrap_or_default(),
    )
    .await
}

pub async fn training_checkins(limit: u32) -> ApiResult<Vec<CheckinRecord>> {
    get_json(&format!("/training/checkins?limit={}", limit)).await
}

// ---- 组卷 ----

pub async fn assemble_paper(req: &AssembleRequest) -> ApiResult<PaperBundle> {
    post_json("/papers", &serde_json::to_string(req).unwrap_or_default()).await
}

pub async fn read_paper(paper_id: i64) -> ApiResult<PaperBundle> {
    get_json(&format!("/papers/{}", paper_id)).await
}

pub async fn submit_paper(paper_id: i64, req: &SubmitRequest) -> ApiResult<PaperResult> {
    post_json(
        &format!("/papers/{}/submit", paper_id),
        &serde_json::to_string(req).unwrap_or_default(),
    )
    .await
}

// ---- 附件 ----

/// 带鉴权获取附件二进制，返回 (mime, bytes)。`<img>`/`<video>` 标签无法
/// 携带 Authorization header，笔记媒体由视图 fetch → Blob → objectURL 后
/// 按 mime 决定赋给 img 还是 video（§8.1 附件读取）。
pub async fn fetch_attachment(id: i64) -> ApiResult<(String, Vec<u8>)> {
    let url = format!("{}/api/v1/attachments/{}", base_url(), id);
    let mut req = Request::get(&url);
    let token = AUTH_TOKEN_OPT.with(|c| c.borrow().clone());
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    let req = Request::try_from(req).map_err(|e| ApiError::Network(e.to_string()))?;
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    let status = resp.status();
    if !(200..300).contains(&status) {
        if status == 401 {
            fire_unauthorized();
        }
        let text = resp
            .text()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        return Err(parse_error(&text).await);
    }
    let mime = resp
        .headers()
        .get("content-type")
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let bytes = resp
        .binary()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    Ok((mime, bytes))
}

/// 上传附件到笔记（raw bytes，Content-Type 白名单 + 魔数嗅探在服务端校验）。
/// 返回附件 id + 读取 URL（服务端响应的 Attachment 结构）。
pub async fn upload_attachment(item_id: i64, filename: &str, bytes: Vec<u8>) -> ApiResult<i64> {
    #[derive(Deserialize)]
    struct Att {
        id: i64,
    }
    let url = format!(
        "{}/api/v1/items/{}/attachments?name={}",
        base_url(),
        item_id,
        filename
    );
    let mut req = Request::post(&url);
    req = req.header("Content-Type", "application/octet-stream");
    let token = AUTH_TOKEN_OPT.with(|c| c.borrow().clone());
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    let req = req
        .body(bytes)
        .map_err(|e| ApiError::Network(e.to_string()))?;
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    let status = resp.status();
    if !(200..300).contains(&status) {
        if status == 401 {
            fire_unauthorized();
        }
        let text = resp
            .text()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        return Err(parse_error(&text).await);
    }
    let text = resp
        .text()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    serde_json::from_str::<Att>(&text)
        .map(|a| a.id)
        .map_err(|e| ApiError::Network(format!("解析失败：{}", e)))
}

/// 视频题作答：提交已上传的训练视频附件 id + 训练想法，不判分，落事件待 AI 复盘。
pub async fn video_answer(question_id: i64, attachment_ids: Vec<i64>, note: Option<String>) -> ApiResult<()> {
    #[derive(Serialize)]
    struct Req {
        question_id: i64,
        attachment_ids: Vec<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    }
    send(
        "POST",
        "/quiz/video-answer",
        Some(&serde_json::to_string(&Req {
            question_id,
            attachment_ids,
            note,
        })
        .unwrap_or_default()),
        true,
    )
    .await
    .map(|_| ())
}

// ---- Agent 凭证 ----

pub async fn credential() -> ApiResult<CredentialResponse> {
    get_json("/agent/credential").await
}

pub async fn rotate_credential() -> ApiResult<CredentialResponse> {
    post_json("/agent/credential/rotate", "{}").await
}
