//! MCP 网关工具实现（docs/architecture.md §8.2：11 个工具 + 强制规则）。
//!
//! 用户身份一律从请求扩展 `AuthUser`（auth.rs 中间件注入）读取——工具入参
//! 中不存在用户身份字段（§10 数据隔离）。错误映射：Invalid/Conflict →
//! invalid_params，NotFound → resource_not_found，Storage → internal_error。
//! Agent 的写入（create_item/write_item）固定经 agent_* 用例落 agent_write
//! 事件供客户端溯源。

use std::collections::BTreeSet;
use std::sync::Arc;

use application::agent::{AgentCapability, AgentStatus, QuestionInput};
use axum::http::request::Parts;
use base64::Engine as _;
use chrono::{DateTime, NaiveDate, Utc};
use domain::error::Error;
use domain::event::{Event, EventAction};
use domain::identity::User;
use domain::practice::{Answer, QuestionType};
use domain::skill::Skill;
use domain::space::{
    Annotation, AnnotationAuthor, Attachment, Creator, Item, ItemKind, ItemNode, Workspace,
};
use rmcp::ErrorData;
use rmcp::handler::server::common::Extension;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{Json, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::McpState;
use crate::auth::AuthUser;

/// MCP 服务主体：应用服务引用 + rmcp 路由表。
pub struct McpService {
    state: Arc<McpState>,
    tool_router: ToolRouter<Self>,
}

impl McpService {
    pub fn new(state: Arc<McpState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// 从请求扩展取回鉴权用户（require_auth 已校验 token 并注入）。
    fn user(&self, parts: &Parts) -> Result<User, ErrorData> {
        parts
            .extensions
            .get::<AuthUser>()
            .map(|a| a.0.clone())
            .ok_or_else(|| ErrorData::internal_error("鉴权上下文缺失", None))
    }
}

/// 领域错误 → MCP 协议错误（§8.2：404→resource_not_found，其余→invalid/internal）。
fn map_err(e: Error) -> ErrorData {
    match e {
        Error::NotFound(msg) => ErrorData::resource_not_found(msg, None),
        Error::Storage(msg) => ErrorData::internal_error(msg, None),
        Error::Invalid(msg) | Error::Conflict(msg) => ErrorData::invalid_params(msg, None),
    }
}

#[tool_router]
impl McpService {
    /// AgentBootstrap：能力包下发（Skill + 备考提示词 + 工具清单 + Skill
    /// 目录全量内容 + 版本号）。内置与用户自定义合并（同名用户覆盖内置）
    /// 全量下发（含脚本），Agent 首次接入即自动下载安装；之后可按名经
    /// get_skill 重新拉取。
    #[tool(
        name = "bootstrap",
        title = "能力下发",
        description = "获取 Agent 能力包：Skill、备考提示词、工具清单、Skill 目录（内置与用户自定义合并，全量含脚本）与版本号。连接后先调用本工具。"
    )]
    async fn bootstrap(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<Json<AgentCapabilityDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .bootstrap(user.id)
            .await
            .map(AgentCapabilityDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// ManageExamGoal（Agent 入口）：创建备考空间。
    #[tool(
        name = "create_workspace",
        title = "创建备考空间",
        description = "创建用户的备考空间，需提供名称与考试目标（日期可选）。"
    )]
    async fn create_workspace(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<CreateWorkspaceInput>,
    ) -> Result<Json<WorkspaceDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .create_workspace(user.id, input.name, input.exam_goal, input.exam_date)
            .await
            .map(WorkspaceDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// 创建目录/笔记（Agent 内容生成入口，落 agent_write 事件）。
    #[tool(
        name = "create_item",
        title = "创建目录或笔记",
        description = "在空间下创建目录（kind=dir）或笔记（kind=note）；parent_id 为空创建根节点，否则父节点必须是目录。"
    )]
    async fn create_item(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<CreateItemInput>,
    ) -> Result<Json<ItemDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .agent_create_item(
                user.id,
                input.workspace_id,
                input.parent_id,
                input.kind.into(),
                input.name,
                input.content,
            )
            .await
            .map(ItemDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// 更新笔记正文（Agent 续写，落 agent_write 事件）。
    #[tool(
        name = "write_item",
        title = "写入笔记正文",
        description = "覆盖更新笔记的 Markdown 正文（目录没有正文，不可写入）。"
    )]
    async fn write_item(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<WriteItemInput>,
    ) -> Result<Json<ItemDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .agent_write_item(user.id, input.item_id, input.content)
            .await
            .map(ItemDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// ReadNote：读取单个节点（目录或笔记）。
    #[tool(
        name = "read_item",
        title = "读取内容",
        description = "按 id 读取目录或笔记（含正文）。"
    )]
    async fn read_item(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<ReadItemInput>,
    ) -> Result<Json<ItemDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .read_item(user.id, input.item_id)
            .await
            .map(ItemDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// BrowseTree：空间完整内容树（供生成规划与进度核对）。
    #[tool(
        name = "list_items",
        title = "浏览内容树",
        description = "返回空间下完整的目录/笔记树。"
    )]
    async fn list_items(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<ListItemsInput>,
    ) -> Result<Json<ItemTreeOutput>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .tree(user.id, input.workspace_id)
            .await
            .map(|items| {
                Json(ItemTreeOutput {
                    items: items.into_iter().map(ItemNodeDto::from).collect(),
                })
            })
            .map_err(map_err)
    }

    /// Annotate（Agent 入口）：在笔记上追加 AI 批注。
    #[tool(
        name = "add_annotation",
        title = "添加批注",
        description = "在笔记正文的锚点处添加 AI 批注（只能加在笔记上）。"
    )]
    async fn add_annotation(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<AddAnnotationInput>,
    ) -> Result<Json<AnnotationDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .annotate(
                user.id,
                input.item_id,
                input.anchor,
                input.text,
                AnnotationAuthor::Ai,
            )
            .await
            .map(AnnotationDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// SaveQuestions：批量写入习题（≤ 200 道，须归属笔记（集））。
    #[tool(
        name = "save_questions",
        title = "保存习题",
        description = "批量保存习题到指定笔记（集），单批不超过 200 道。"
    )]
    async fn save_questions(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<SaveQuestionsInput>,
    ) -> Result<Json<SavedQuestionsOutput>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .save_questions(
                user.id,
                input.workspace_id,
                input.source_item_id,
                input
                    .questions
                    .into_iter()
                    .map(QuestionInput::from)
                    .collect(),
            )
            .await
            .map(|ids| Json(SavedQuestionsOutput { ids }))
            .map_err(map_err)
    }

    /// ReadEvents：按用户回放最近行为（新→旧）。
    #[tool(
        name = "get_events",
        title = "读取事件",
        description = "返回用户的最近行为事件（新→旧），默认 20 条，limit 可调。"
    )]
    async fn get_events(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<GetEventsInput>,
    ) -> Result<Json<EventsOutput>, ErrorData> {
        let user = self.user(&parts)?;
        let limit = input.limit.unwrap_or(20);
        self.state
            .agent
            .read_events(user.id, limit)
            .await
            .map(|events| {
                Json(EventsOutput {
                    events: events.into_iter().map(EventDto::from).collect(),
                })
            })
            .map_err(map_err)
    }

    /// ReportStatus：空间列表 + 最近行为，客户端据此刷新生成状态。
    #[tool(
        name = "report_status",
        title = "报告状态",
        description = "返回用户的空间列表与最近行为，用于刷新 AI 生成进度。"
    )]
    async fn report_status(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<Json<AgentStatusDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .report_status(user.id)
            .await
            .map(AgentStatusDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// GetSkill：按名拉取 skill 完整内容（含脚本）。用户自定义优先，
    /// 无同名自定义时回退系统内置目录。重新安装/更新用。
    #[tool(
        name = "get_skill",
        title = "拉取 Skill",
        description = "按名称拉取 skill 的完整内容（含脚本）：用户自定义优先，无同名自定义时回退系统内置目录。用于重新安装或更新已安装的 skill。"
    )]
    async fn get_skill(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<GetSkillInput>,
    ) -> Result<Json<SkillDto>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .get_skill(user.id, &input.name)
            .await
            .map(SkillDto::from)
            .map(Json)
            .map_err(map_err)
    }

    /// UploadAttachment（Agent 入口）：笔记图片上传。二进制经 base64 传入，
    /// 服务端解码后走与 REST 相同的校验（魔数嗅探 + 10MB 上限 + 归属）。
    #[tool(
        name = "upload_attachment",
        title = "上传笔记图片",
        description = "向笔记上传图片附件（png/jpeg/gif/webp，≤10MB）：二进制以 base64 传入，服务端做魔数嗅探与归属校验。返回附件 id 与读取 URL（/api/v1/attachments/{id}，带登录态访问）。用于在笔记正文中以 ![alt](url) 引用图片。"
    )]
    async fn upload_attachment(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(input): Parameters<UploadAttachmentInput>,
    ) -> Result<Json<AttachmentDto>, ErrorData> {
        let user = self.user(&parts)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&input.content_base64)
            .map_err(|_| map_err(Error::Invalid("图片内容 base64 解码失败".to_owned())))?;
        self.state
            .attachments
            .upload(user.id, input.item_id, input.filename, &bytes)
            .await
            .map(AttachmentDto::from)
            .map(Json)
            .map_err(map_err)
    }
}

#[tool_handler]
impl ServerHandler for McpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::default(),
            server_info: Implementation {
                name: "xueban-mcp".to_owned(),
                title: Some("学伴 MCP 网关".to_owned()),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                description: Some(
                    "学伴备考助手的 MCP 接入网关：目录生成、笔记写作、习题保存。".to_owned(),
                ),
                ..Default::default()
            },
            instructions: Some(
                "你是学伴备考助手。先调用 bootstrap 获取能力包，再按用户指令生成内容；\
                 所有工具都会校验用户身份与归属，无需在参数中携带用户信息。"
                    .to_owned(),
            ),
        }
    }
}

/// create_workspace 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateWorkspaceInput {
    pub name: String,
    pub exam_goal: String,
    pub exam_date: Option<NaiveDate>,
}

/// create_item 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateItemInput {
    pub workspace_id: i64,
    pub parent_id: Option<i64>,
    pub kind: ItemKindDto,
    pub name: String,
    pub content: Option<String>,
}

/// write_item 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteItemInput {
    pub item_id: i64,
    pub content: String,
}

/// read_item 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadItemInput {
    pub item_id: i64,
}

/// list_items 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListItemsInput {
    pub workspace_id: i64,
}

/// add_annotation 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AddAnnotationInput {
    pub item_id: i64,
    /// 正文引用片段（定位锚点）。
    pub anchor: String,
    pub text: String,
}

/// save_questions 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SaveQuestionsInput {
    pub workspace_id: i64,
    /// 题源笔记（集）id：题目必须归属到笔记（集）。
    pub source_item_id: i64,
    pub questions: Vec<QuestionInputDto>,
}

/// get_events 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetEventsInput {
    /// 返回条数（默认 20）。
    pub limit: Option<u32>,
}

/// list_items 出参（rmcp 要求工具输出 schema 根类型为 object，数组须包装）。
#[derive(Debug, Serialize, JsonSchema)]
pub struct ItemTreeOutput {
    pub items: Vec<ItemNodeDto>,
}

/// save_questions 出参：新写入的题目 id 列表。
#[derive(Debug, Serialize, JsonSchema)]
pub struct SavedQuestionsOutput {
    pub ids: Vec<i64>,
}

/// get_events 出参。
#[derive(Debug, Serialize, JsonSchema)]
pub struct EventsOutput {
    pub events: Vec<EventDto>,
}

/// P2-13：协议层 schema 关注点收口在 adapter-mcp（rmcp 要求工具入参/出参实现
/// JsonSchema，孤儿规则禁止为 domain 类型实现）。以下 DTO 与领域/应用类型
/// 逐字段镜像，serde 属性一致（snake_case / untagged），wire 格式与先前完全
/// 不变；domain 与 application 不再依赖 schemars。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceDto {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub exam_goal: String,
    pub exam_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemKindDto {
    Dir,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreatorDto {
    Agent,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ItemDto {
    pub id: i64,
    pub workspace_id: i64,
    pub parent_id: Option<i64>,
    pub kind: ItemKindDto,
    pub name: String,
    pub content: Option<String>,
    pub created_by: CreatorDto,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ItemNodeDto {
    pub item: ItemDto,
    pub children: Vec<ItemNodeDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationAuthorDto {
    Ai,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AnnotationDto {
    pub id: i64,
    pub item_id: i64,
    pub user_id: i64,
    pub author: AnnotationAuthorDto,
    pub anchor: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventActionDto {
    Annotate,
    Answer,
    Wrong,
    AgentWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventDto {
    pub id: i64,
    pub user_id: i64,
    pub workspace_id: Option<i64>,
    pub item_id: Option<i64>,
    pub action: EventActionDto,
    pub payload: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionTypeDto {
    Single,
    Multi,
    Judge,
}

/// 线格式与 domain::practice::Answer 一致（untagged，§8.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AnswerDto {
    Single(usize),
    Multi(BTreeSet<usize>),
    Judge(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QuestionInputDto {
    pub qtype: QuestionTypeDto,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: AnswerDto,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentCapabilityDto {
    pub assistant: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub skills: Vec<SkillDto>,
    pub version: u32,
}

/// 内置 Skill：bootstrap 全量下发、get_skill 按名拉取，同一结构。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillDto {
    pub name: String,
    pub description: String,
    pub script: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct GetSkillInput {
    pub name: String,
}

/// upload_attachment 入参：二进制以 base64 传输（JSON-RPC 无二进制通道）。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UploadAttachmentInput {
    /// 目标笔记 id（仅 note 可挂附件）。
    pub item_id: i64,
    /// 原始文件名（仅展示用；缺省按嗅探 mime 给默认）。
    pub filename: String,
    /// 图片文件内容的 base64（标准编码；解码后 ≤10MB）。
    pub content_base64: String,
}

/// upload_attachment 出参：附件 id + 读取 URL（前端 fetch-blob 渲染用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AttachmentDto {
    pub id: i64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentStatusDto {
    pub workspaces: Vec<WorkspaceDto>,
    pub recent_events: Vec<EventDto>,
}

// ---- DTO ↔ 领域类型转换（仅实现工具路径实际用到的方向） ----

impl From<ItemKindDto> for ItemKind {
    fn from(v: ItemKindDto) -> Self {
        match v {
            ItemKindDto::Dir => ItemKind::Dir,
            ItemKindDto::Note => ItemKind::Note,
        }
    }
}

impl From<ItemKind> for ItemKindDto {
    fn from(v: ItemKind) -> Self {
        match v {
            ItemKind::Dir => ItemKindDto::Dir,
            ItemKind::Note => ItemKindDto::Note,
        }
    }
}

impl From<Creator> for CreatorDto {
    fn from(v: Creator) -> Self {
        match v {
            Creator::Agent => CreatorDto::Agent,
            Creator::User => CreatorDto::User,
        }
    }
}

impl From<Item> for ItemDto {
    fn from(v: Item) -> Self {
        Self {
            id: v.id,
            workspace_id: v.workspace_id,
            parent_id: v.parent_id,
            kind: v.kind.into(),
            name: v.name,
            content: v.content,
            created_by: v.created_by.into(),
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

impl From<ItemNode> for ItemNodeDto {
    fn from(v: ItemNode) -> Self {
        Self {
            item: v.item.into(),
            children: v.children.into_iter().map(Self::from).collect(),
        }
    }
}

impl From<Workspace> for WorkspaceDto {
    fn from(v: Workspace) -> Self {
        Self {
            id: v.id,
            user_id: v.user_id,
            name: v.name,
            exam_goal: v.exam_goal,
            exam_date: v.exam_date,
            created_at: v.created_at,
        }
    }
}

impl From<AnnotationAuthor> for AnnotationAuthorDto {
    fn from(v: AnnotationAuthor) -> Self {
        match v {
            AnnotationAuthor::Ai => AnnotationAuthorDto::Ai,
            AnnotationAuthor::User => AnnotationAuthorDto::User,
        }
    }
}

impl From<Annotation> for AnnotationDto {
    fn from(v: Annotation) -> Self {
        Self {
            id: v.id,
            item_id: v.item_id,
            user_id: v.user_id,
            author: v.author.into(),
            anchor: v.anchor,
            text: v.text,
            created_at: v.created_at,
        }
    }
}

impl From<EventAction> for EventActionDto {
    fn from(v: EventAction) -> Self {
        match v {
            EventAction::Annotate => EventActionDto::Annotate,
            EventAction::Answer => EventActionDto::Answer,
            EventAction::Wrong => EventActionDto::Wrong,
            EventAction::AgentWrite => EventActionDto::AgentWrite,
        }
    }
}

impl From<Event> for EventDto {
    fn from(v: Event) -> Self {
        Self {
            id: v.id,
            user_id: v.user_id,
            workspace_id: v.workspace_id,
            item_id: v.item_id,
            action: v.action.into(),
            payload: v.payload,
            created_at: v.created_at,
        }
    }
}

impl From<QuestionTypeDto> for QuestionType {
    fn from(v: QuestionTypeDto) -> Self {
        match v {
            QuestionTypeDto::Single => QuestionType::Single,
            QuestionTypeDto::Multi => QuestionType::Multi,
            QuestionTypeDto::Judge => QuestionType::Judge,
        }
    }
}

impl From<AnswerDto> for Answer {
    fn from(v: AnswerDto) -> Self {
        match v {
            AnswerDto::Single(i) => Answer::Single(i),
            AnswerDto::Multi(s) => Answer::Multi(s),
            AnswerDto::Judge(b) => Answer::Judge(b),
        }
    }
}

impl From<QuestionInputDto> for QuestionInput {
    fn from(v: QuestionInputDto) -> Self {
        Self {
            qtype: v.qtype.into(),
            stem: v.stem,
            options: v.options,
            answer: v.answer.into(),
            explanation: v.explanation,
        }
    }
}

impl From<Skill> for SkillDto {
    fn from(v: Skill) -> Self {
        Self {
            name: v.name,
            description: v.description,
            script: v.script,
        }
    }
}

impl From<AgentCapability> for AgentCapabilityDto {
    fn from(v: AgentCapability) -> Self {
        Self {
            assistant: v.assistant,
            prompt: v.prompt,
            tools: v.tools,
            skills: v.skills.into_iter().map(SkillDto::from).collect(),
            version: v.version,
        }
    }
}

impl From<AgentStatus> for AgentStatusDto {
    fn from(v: AgentStatus) -> Self {
        Self {
            workspaces: v.workspaces.into_iter().map(WorkspaceDto::from).collect(),
            recent_events: v.recent_events.into_iter().map(EventDto::from).collect(),
        }
    }
}

impl From<Attachment> for AttachmentDto {
    fn from(v: Attachment) -> Self {
        Self {
            id: v.id,
            url: format!("/api/v1/attachments/{}", v.id),
        }
    }
}
