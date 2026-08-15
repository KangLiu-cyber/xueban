//! Agent 接入用例：AgentBootstrap / SaveQuestions / ReadEvents / ReportStatus。
//!
//! Agent 只经 MCP 接入：连接时 token 解析出 user_id 注入上下文，工具入参
//! 不存在用户身份字段。能力包（Skill + 提示词 + 工具清单）按版本下发；
//! Agent 写入（save_questions）复用题目仓储，落 agent_write 事件供客户端溯源。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::{EventStore, ItemRepository, QuestionRepository, WorkspaceRepository};
use domain::practice::{Answer, Question, QuestionType};
use domain::space::Workspace;
use serde::{Deserialize, Serialize};

/// 单批题目上限（协议层校验，与仓储端口约定一致）。
pub const MAX_QUESTIONS_PER_BATCH: usize = 200;

/// 能力包版本：服务端升级能力后递增，Agent 下次接入自动获取新版本。
pub const CAPABILITY_VERSION: u32 = 1;

/// Agent 提交的题目入参（无 id/归属字段，归属由上下文与调用参数给定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuestionInput {
    pub qtype: QuestionType,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: Answer,
    pub explanation: Option<String>,
}

/// AgentBootstrap 返回值：Skill 定义、备考提示词、工具清单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentCapability {
    pub skill: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub version: u32,
}

/// ReportStatus 返回值：客户端刷新"AI 生成进度"用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentStatus {
    pub workspaces: Vec<Workspace>,
    /// 最近行为（新→旧）。
    pub recent_events: Vec<Event>,
}

/// MCP 工具清单（与 docs/architecture.md §8.2 对齐）。
const TOOLS: [&str; 10] = [
    "bootstrap",
    "create_workspace",
    "create_item",
    "write_item",
    "read_item",
    "list_items",
    "add_annotation",
    "save_questions",
    "get_events",
    "report_status",
];

pub struct AgentService<W, I, Q, E>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    E: EventStore + ?Sized,
{
    workspaces: Arc<W>,
    items: Arc<I>,
    questions: Arc<Q>,
    events: Arc<E>,
}

impl<W, I, Q, E> AgentService<W, I, Q, E>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    E: EventStore + ?Sized,
{
    pub fn new(workspaces: Arc<W>, items: Arc<I>, questions: Arc<Q>, events: Arc<E>) -> Self {
        Self {
            workspaces,
            items,
            questions,
            events,
        }
    }

    /// AgentBootstrap：能力下发。提示词基于用户首个空间的考试目标定制；
    /// 尚无空间时给出引导文案，能力本身始终可用。
    pub async fn bootstrap(&self, user_id: i64) -> Result<AgentCapability> {
        let goal = self
            .workspaces
            .list_by_user(user_id)
            .await?
            .into_iter()
            .next()
            .map(|w| w.exam_goal);
        let prompt = match goal {
            Some(goal) if !goal.trim().is_empty() => format!(
                "你是学伴备考助手，为用户的备考空间生成学习内容。\n用户考试目标：{goal}\n\
                 请先 bootstrap 获取工具清单，再按用户指令生成目录、笔记与习题。"
            ),
            _ => "你是学伴备考助手。用户尚未设置考试目标：请先引导创建备考空间 \
                  （create_workspace），再按指令生成内容。"
                .to_owned(),
        };
        Ok(AgentCapability {
            skill: "xueban-study-assistant".to_owned(),
            prompt,
            tools: TOOLS.iter().map(|t| (*t).to_owned()).collect(),
            version: CAPABILITY_VERSION,
        })
    }

    /// SaveQuestions：批量写入习题（单批 ≤ 200），归属校验 + 笔记（集）约束，
    /// 落 agent_write 事件。
    pub async fn save_questions(
        &self,
        user_id: i64,
        workspace_id: i64,
        source_item_id: i64,
        questions: Vec<QuestionInput>,
    ) -> Result<Vec<i64>> {
        if questions.is_empty() {
            return Err(Error::Invalid("题目列表不能为空".to_owned()));
        }
        if questions.len() > MAX_QUESTIONS_PER_BATCH {
            return Err(Error::Invalid(format!(
                "单批题目不能超过 {MAX_QUESTIONS_PER_BATCH} 道"
            )));
        }
        self.workspaces
            .find_by_id_and_user(workspace_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("备考空间不存在".to_owned()))?;
        let item = self
            .items
            .find_by_id(source_item_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("内容不存在".to_owned()))?;
        // item 不存 user_id：归属经所属空间校验（workspace_id 已验为用户所有）。
        if item.workspace_id != workspace_id {
            return Err(Error::NotFound("内容不存在".to_owned()));
        }
        if !item.is_note() {
            return Err(Error::Invalid("题目必须归属到笔记（集）".to_owned()));
        }
        let now = Utc::now();
        let rows: Vec<Question> = questions
            .into_iter()
            .map(|q| Question {
                id: 0,
                workspace_id,
                source_item_id,
                qtype: q.qtype,
                stem: q.stem,
                options: q.options,
                answer: q.answer,
                explanation: q.explanation,
                created_at: now,
            })
            .collect();
        let ids = self.questions.insert_many(&rows).await?;
        self.events
            .append(&Event {
                id: 0,
                user_id,
                workspace_id: Some(workspace_id),
                item_id: Some(source_item_id),
                action: EventAction::AgentWrite,
                payload: Some(
                    serde_json::json!({
                        "workspace_id": workspace_id,
                        "item_id": source_item_id,
                        "question_count": ids.len(),
                    })
                    .to_string(),
                ),
                created_at: now,
            })
            .await?;
        Ok(ids)
    }

    /// ReadEvents：按用户回放最近行为（新→旧）。
    pub async fn read_events(&self, user_id: i64, limit: u32) -> Result<Vec<Event>> {
        let mut events = self.events.list_by_user(user_id, limit).await?;
        events.reverse(); // 仓储升序返回，此处转新→旧供 Agent 展示。
        Ok(events)
    }

    /// ReportStatus：空间列表 + 最近行为，客户端据此刷新生成状态。
    pub async fn report_status(&self, user_id: i64) -> Result<AgentStatus> {
        let workspaces = self.workspaces.list_by_user(user_id).await?;
        let recent_events = self.read_events(user_id, 20).await?;
        Ok(AgentStatus {
            workspaces,
            recent_events,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryEventStore, InMemoryItemRepository, InMemoryQuestionRepository,
        InMemoryWorkspaceRepository, insert_item,
    };
    use domain::space::ItemKind;

    struct Ctx {
        svc: AgentService<
            InMemoryWorkspaceRepository,
            InMemoryItemRepository,
            InMemoryQuestionRepository,
            InMemoryEventStore,
        >,
        ws_id: i64,
        item_id: i64,
        item_repo: Arc<InMemoryItemRepository>,
    }

    async fn ctx() -> Ctx {
        let ws_repo = Arc::new(InMemoryWorkspaceRepository::default());
        let item_repo = Arc::new(InMemoryItemRepository::default());
        let ws = Workspace {
            id: 0,
            user_id: 1,
            name: "备考".into(),
            exam_goal: "软考架构师".into(),
            exam_date: None,
            created_at: Utc::now(),
        };
        let ws_id = ws_repo.insert(&ws).await.unwrap();
        let item_id = insert_item(&item_repo, ws_id, "第1集", ItemKind::Note).await;
        let svc = AgentService::new(
            ws_repo,
            item_repo.clone(),
            Arc::new(InMemoryQuestionRepository::default()),
            Arc::new(InMemoryEventStore::default()),
        );
        Ctx {
            svc,
            ws_id,
            item_id,
            item_repo,
        }
    }

    fn input() -> QuestionInput {
        QuestionInput {
            qtype: QuestionType::Single,
            stem: "1+1=?".into(),
            options: vec!["1".into(), "2".into()],
            answer: Answer::Single(1),
            explanation: Some("算术".into()),
        }
    }

    #[tokio::test]
    async fn bootstrap_returns_capability_with_goal_and_tools() {
        let c = ctx().await;
        let cap = c.svc.bootstrap(1).await.unwrap();
        assert_eq!(cap.version, CAPABILITY_VERSION);
        assert!(cap.prompt.contains("软考架构师"));
        assert_eq!(cap.tools.len(), 10);
        assert!(cap.tools.contains(&"save_questions".to_owned()));
    }

    #[tokio::test]
    async fn bootstrap_without_workspace_guides_creation() {
        let ws_repo = Arc::new(InMemoryWorkspaceRepository::default());
        let svc = AgentService::new(
            ws_repo,
            Arc::new(InMemoryItemRepository::default()),
            Arc::new(InMemoryQuestionRepository::default()),
            Arc::new(InMemoryEventStore::default()),
        );
        let cap = svc.bootstrap(2).await.unwrap();
        assert!(cap.prompt.contains("create_workspace"));
    }

    #[tokio::test]
    async fn save_questions_writes_ids_and_agent_write_event() {
        let c = ctx().await;
        let ids = c
            .svc
            .save_questions(1, c.ws_id, c.item_id, vec![input(), input()])
            .await
            .unwrap();
        assert_eq!(ids.len(), 2);
        // 事件：agent_write，payload 携带题数。
        let events = c.svc.read_events(1, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::AgentWrite);
        assert_eq!(events[0].item_id, Some(c.item_id));
        assert!(
            events[0]
                .payload
                .as_deref()
                .unwrap()
                .contains("question_count")
        );
        // 题目确实落库且归属正确。
        let qs = c.svc.questions.find_by_ids(&ids, 1).await.unwrap();
        assert_eq!(qs.len(), 2);
        assert!(qs.iter().all(|q| q.source_item_id == c.item_id));
    }

    #[tokio::test]
    async fn save_questions_rejects_batch_size_and_ownership() {
        let c = ctx().await;
        // 空批与超批。
        assert!(matches!(
            c.svc.save_questions(1, c.ws_id, c.item_id, vec![]).await,
            Err(Error::Invalid(_))
        ));
        let too_many: Vec<QuestionInput> = (0..=MAX_QUESTIONS_PER_BATCH).map(|_| input()).collect();
        assert!(matches!(
            c.svc.save_questions(1, c.ws_id, c.item_id, too_many).await,
            Err(Error::Invalid(_))
        ));
        // 跨用户空间/内容。
        assert!(
            c.svc
                .save_questions(2, c.ws_id, c.item_id, vec![input()])
                .await
                .is_err()
        );
        assert!(
            c.svc
                .save_questions(1, c.ws_id, 999, vec![input()])
                .await
                .is_err()
        );
        // 他空间（他人/自己的其他空间）的笔记不可作来源。
        let ws2 = Workspace {
            id: 0,
            user_id: 1,
            name: "另一空间".into(),
            exam_goal: "目标".into(),
            exam_date: None,
            created_at: Utc::now(),
        };
        let ws2_id = c.svc.workspaces.insert(&ws2).await.unwrap();
        let foreign_item = insert_item(&c.item_repo, ws2_id, "外集", ItemKind::Note).await;
        assert!(
            c.svc
                .save_questions(1, c.ws_id, foreign_item, vec![input()])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn save_questions_rejects_non_note_item() {
        let c = ctx().await;
        let dir_id = insert_item(&c.item_repo, c.ws_id, "目录", ItemKind::Dir).await;
        assert!(matches!(
            c.svc
                .save_questions(1, c.ws_id, dir_id, vec![input()])
                .await,
            Err(Error::Invalid(_))
        ));
        // 校验失败不落任何事件。
        let events = c.svc.events.list_by_user(1, 10).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn read_events_returns_newest_first() {
        let c = ctx().await;
        c.svc
            .save_questions(1, c.ws_id, c.item_id, vec![input()])
            .await
            .unwrap();
        let _ = c
            .svc
            .save_questions(1, c.ws_id, c.item_id, vec![input()])
            .await
            .unwrap();
        let events = c.svc.read_events(1, 10).await.unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].created_at >= events[1].created_at);
    }

    #[tokio::test]
    async fn report_status_summarizes_workspaces_and_events() {
        let c = ctx().await;
        c.svc
            .save_questions(1, c.ws_id, c.item_id, vec![input()])
            .await
            .unwrap();
        let status = c.svc.report_status(1).await.unwrap();
        assert_eq!(status.workspaces.len(), 1);
        assert_eq!(status.workspaces[0].name, "备考");
        assert_eq!(status.recent_events.len(), 1);
        // 跨用户隔离。
        let other = c.svc.report_status(2).await.unwrap();
        assert!(other.workspaces.is_empty() && other.recent_events.is_empty());
    }
}
