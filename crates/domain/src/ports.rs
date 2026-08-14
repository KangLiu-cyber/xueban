//! 输出端口：仓储与基础设施 trait（被驱动适配器实现）。
//!
//! 与 docs/architecture.md §5.2 端口清单一致。仓储查询必须携带
//! user_id/workspace_id 归属条件——实现侧在 SQL 层强制，作为隔离第二道防线。
//!
//! 仓储方法均为原生 `async fn in trait`（Rust 2024 稳定特性）。
//! 调用方只在 request 处理路径直接 await，从不跨线程 spawn，
//! 故不要求 future 为 Send，此处统一允许该 lint。

#![allow(async_fn_in_trait)]

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::Result;
use crate::event::Event;
use crate::identity::{Token, TokenPurpose, User};
use crate::practice::{Paper, PaperConfig, Question, QuestionType, QuizRecord, WrongItem};
use crate::space::{Annotation, AnnotationAuthor, Item, ItemNode, Workspace};

pub trait UserRepository {
    /// 插入用户，返回落库后的新 id。
    async fn insert(&self, user: &User) -> Result<i64>;
    async fn find_by_account(&self, account: &str) -> Result<Option<User>>;
    async fn find_by_id(&self, id: i64) -> Result<Option<User>>;
}

pub trait TokenRepository {
    /// 插入 token，返回落库后的新 id。
    async fn insert(&self, token: &Token) -> Result<i64>;
    /// 按凭证查 token（含已吊销的，由调用方判断状态）。
    async fn find_by_token(&self, token: &str) -> Result<Option<Token>>;
    /// 查用户某用途的现行凭证（未吊销，取最新一条）。
    async fn find_active_by_user_purpose(
        &self,
        user_id: i64,
        purpose: TokenPurpose,
    ) -> Result<Option<Token>>;
    /// 吊销单个 token。
    async fn revoke(&self, token: &str, now: DateTime<Utc>) -> Result<()>;
    /// 吊销某用户某用途的全部 token（凭证换发/注销时用）。
    async fn revoke_by_user_purpose(
        &self,
        user_id: i64,
        purpose: TokenPurpose,
        now: DateTime<Utc>,
    ) -> Result<()>;
}

pub trait WorkspaceRepository {
    /// 插入空间，返回落库后的新 id。
    async fn insert(&self, ws: &Workspace) -> Result<i64>;
    /// 按归属查单个空间。
    async fn find_by_id_and_user(&self, id: i64, user_id: i64) -> Result<Option<Workspace>>;
    /// 用户的全部空间。
    async fn list_by_user(&self, user_id: i64) -> Result<Vec<Workspace>>;
    /// 更新空间信息（ManageExamGoal）。
    async fn update(&self, ws: &Workspace) -> Result<()>;
}

pub trait ItemRepository {
    /// 插入节点，返回落库后的新 id。
    async fn insert(&self, item: &Item) -> Result<i64>;
    async fn update(&self, item: &Item) -> Result<()>;
    /// 按归属查单个节点。
    async fn find_by_id(&self, id: i64, user_id: i64) -> Result<Option<Item>>;
    /// 查某节点的直接子节点。
    async fn list_children(&self, workspace_id: i64, parent_id: Option<i64>) -> Result<Vec<Item>>;
    /// 查完整树（adapter 用 WITH RECURSIVE 组装为 ItemNode）。
    async fn list_tree(&self, workspace_id: i64, user_id: i64) -> Result<Vec<ItemNode>>;
    /// 取某节点的祖先链（从根到自身，含自身），防环校验用。
    async fn ancestors(&self, item_id: i64) -> Result<Vec<i64>>;
}

pub trait AnnotationRepository {
    /// 插入批注，返回落库后的新 id。
    async fn insert(&self, ann: &Annotation) -> Result<i64>;
    /// 按笔记列出批注。
    async fn list_by_item(&self, item_id: i64) -> Result<Vec<Annotation>>;
    /// 删除批注（须带归属校验：item 属于 user）。
    async fn delete(&self, id: i64, user_id: i64) -> Result<bool>;
}

pub trait QuestionRepository {
    /// 批量写入题目（单批 ≤ 200，协议层校验），返回落库后的 id 列表。
    async fn insert_many(&self, questions: &[Question]) -> Result<Vec<i64>>;
    /// 按 id + 归属取题（判分/组卷回填用）。
    async fn find_by_ids(&self, ids: &[i64], user_id: i64) -> Result<Vec<Question>>;
    /// 按范围抽题：workspace 内按来源节点/题型筛选，返回最多 count 题。
    async fn draw(
        &self,
        workspace_id: i64,
        source_item_ids: &[i64],
        qtypes: &[QuestionType],
        count: u32,
    ) -> Result<Vec<Question>>;
}

pub trait QuizRecordRepository {
    /// 只追加一条作答记录，返回落库后的新 id。
    async fn append(&self, record: &QuizRecord) -> Result<i64>;
}

pub trait WrongItemRepository {
    /// 按 (user, question) 查错题。
    async fn find(&self, user_id: i64, question_id: i64) -> Result<Option<WrongItem>>;
    /// 记一次答错（times += 1, mastered = false）；不存在则新建（times = 1）。
    async fn record_mistake(
        &self,
        user_id: i64,
        question_id: i64,
        now: DateTime<Utc>,
    ) -> Result<WrongItem>;
    /// 显式标记掌握。
    async fn mark_mastered(
        &self,
        user_id: i64,
        question_id: i64,
        now: DateTime<Utc>,
    ) -> Result<bool>;
    /// 未掌握错题列表。
    async fn list_unmastered(&self, user_id: i64) -> Result<Vec<WrongItem>>;
}

pub trait PaperRepository {
    /// 插入试卷（含题目快照），返回落库后的新 id。
    async fn insert(&self, paper: &Paper) -> Result<i64>;
    /// 按归属查试卷（题目快照已冻结，随结果一起读）。
    async fn find_by_id_and_user(&self, id: i64, user_id: i64) -> Result<Option<Paper>>;
    /// 交卷后写结果。
    async fn submit(&self, paper: &Paper) -> Result<()>;
}

pub trait EventStore {
    /// 追加一条事件。
    async fn append(&self, event: &Event) -> Result<i64>;
    /// 按用户回放（新→旧），limit 限制条数。
    async fn list_by_user(&self, user_id: i64, limit: u32) -> Result<Vec<Event>>;
}

/// 密码哈希端口：argon2id 实现由被驱动适配器提供。
pub trait PasswordHasher {
    fn hash(&self, plain: &str) -> Result<String>;
    fn verify(&self, plain: &str, hash: &str) -> bool;
}

/// 凭证签发端口：随机 32 字节 Base62，usr_ 前缀（安全章节要求）。
pub trait CredentialIssuer {
    fn issue(&self) -> String;
}

// ---- 领域组装输入（无 id 的新建值），供各仓储 insert 使用 ----

#[derive(Debug, Clone)]
pub struct NewWorkspace {
    pub user_id: i64,
    pub name: String,
    pub exam_goal: String,
    pub exam_date: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
pub struct NewItem {
    pub workspace_id: i64,
    pub parent_id: Option<i64>,
    pub kind: crate::space::ItemKind,
    pub name: String,
    pub content: Option<String>,
    pub created_by: crate::space::Creator,
}

#[derive(Debug, Clone)]
pub struct NewAnnotation {
    pub item_id: i64,
    pub user_id: i64,
    pub author: AnnotationAuthor,
    pub anchor: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct NewQuestion {
    pub workspace_id: i64,
    pub source_item_id: i64,
    pub qtype: QuestionType,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: crate::practice::Answer,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewPaper {
    pub user_id: i64,
    pub workspace_id: i64,
    pub name: Option<String>,
    pub config: PaperConfig,
    pub question_ids: Vec<i64>,
}
