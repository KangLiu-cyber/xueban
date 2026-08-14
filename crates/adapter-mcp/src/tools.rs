//! MCP 网关工具实现（docs/architecture.md §8.2：10 个工具 + 强制规则）。
//!
//! 用户身份一律从请求扩展 `AuthUser`（auth.rs 中间件注入）读取——工具入参
//! 中不存在用户身份字段（§10 数据隔离）。错误映射：Invalid/Conflict →
//! invalid_params，NotFound → resource_not_found，Storage → internal_error。
//! Agent 的写入（create_item/write_item）固定经 agent_* 用例落 agent_write
//! 事件供客户端溯源。

use std::sync::Arc;

use application::agent::{AgentCapability, AgentStatus, QuestionInput};
use axum::http::request::Parts;
use chrono::NaiveDate;
use domain::error::Error;
use domain::event::Event;
use domain::identity::User;
use domain::space::{Annotation, AnnotationAuthor, Item, ItemKind, ItemNode, Workspace};
use rmcp::ErrorData;
use rmcp::handler::server::common::Extension;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{Json, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

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
    /// AgentBootstrap：能力包下发（Skill + 备考提示词 + 工具清单 + 版本号）。
    #[tool(
        name = "bootstrap",
        title = "能力下发",
        description = "获取 Agent 能力包：Skill、备考提示词、工具清单与版本号。连接后先调用本工具。"
    )]
    async fn bootstrap(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<Json<AgentCapability>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .bootstrap(user.id)
            .await
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
    ) -> Result<Json<Workspace>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .create_workspace(user.id, input.name, input.exam_goal, input.exam_date)
            .await
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
    ) -> Result<Json<Item>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .agent_create_item(
                user.id,
                input.workspace_id,
                input.parent_id,
                input.kind,
                input.name,
                input.content,
            )
            .await
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
    ) -> Result<Json<Item>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .agent_write_item(user.id, input.item_id, input.content)
            .await
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
    ) -> Result<Json<Item>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .read_item(user.id, input.item_id)
            .await
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
    ) -> Result<Json<Vec<ItemNode>>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .space
            .tree(user.id, input.workspace_id)
            .await
            .map(Json)
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
    ) -> Result<Json<Annotation>, ErrorData> {
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
    ) -> Result<Json<Vec<i64>>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .save_questions(
                user.id,
                input.workspace_id,
                input.source_item_id,
                input.questions,
            )
            .await
            .map(Json)
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
    ) -> Result<Json<Vec<Event>>, ErrorData> {
        let user = self.user(&parts)?;
        let limit = input.limit.unwrap_or(20);
        self.state
            .agent
            .read_events(user.id, limit)
            .await
            .map(Json)
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
    ) -> Result<Json<AgentStatus>, ErrorData> {
        let user = self.user(&parts)?;
        self.state
            .agent
            .report_status(user.id)
            .await
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
    pub kind: ItemKind,
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
    pub questions: Vec<QuestionInput>,
}

/// get_events 入参。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetEventsInput {
    /// 返回条数（默认 20）。
    pub limit: Option<u32>,
}
