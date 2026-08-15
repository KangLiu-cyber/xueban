//! Agent 接入用例：AgentBootstrap / SaveQuestions / ReadEvents / ReportStatus / GetSkill，
//! 以及用户自定义 Skill 管理（CreateSkill / ListSkills / DeleteSkill）。
//!
//! Agent 只经 MCP 接入：连接时 token 解析出 user_id 注入上下文，工具入参
//! 不存在用户身份字段。能力包（Skill + 提示词 + 工具清单）按版本下发；
//! 系统内置 Skill 目录（开发者放在 `skills/` 文件夹，启动时加载）与用户
//! 自定义 Skill（skills 表，按用户隔离）合并后随 bootstrap 全量下发，
//! 同名用户自定义覆盖内置；之后可按名经 get_skill 重新拉取（用户优先）。
//! Agent 写入（save_questions）复用题目仓储，落 agent_write 事件供客户端溯源。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::{
    EventStore, ItemRepository, QuestionRepository, SkillRepository, WorkspaceRepository,
};
use domain::practice::{Answer, Question, QuestionType};
use domain::skill::{Skill, UserSkill};
use domain::space::Workspace;
use serde::{Deserialize, Serialize};

/// 单批题目上限（协议层校验，与仓储端口约定一致）。
pub const MAX_QUESTIONS_PER_BATCH: usize = 200;

/// 能力包版本：服务端升级能力后递增，Agent 下次接入自动获取新版本。
pub const CAPABILITY_VERSION: u32 = 2;

/// Agent 提交的题目入参（无 id/归属字段，归属由上下文与调用参数给定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionInput {
    pub qtype: QuestionType,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: Answer,
    pub explanation: Option<String>,
}

/// AgentBootstrap 返回值：Skill 定义、备考提示词、工具清单、合并 Skill 目录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub assistant: String,
    pub prompt: String,
    pub tools: Vec<String>,
    /// 合并 Skill 目录（全量下发，含脚本）：系统内置 + 用户自定义，同名
    /// 用户自定义覆盖内置；Agent 首次接入自动下载安装。
    pub skills: Vec<Skill>,
    pub version: u32,
}

/// ReportStatus 返回值：客户端刷新"AI 生成进度"用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatus {
    pub workspaces: Vec<Workspace>,
    /// 最近行为（新→旧）。
    pub recent_events: Vec<Event>,
}

/// MCP 工具清单（与 docs/architecture.md §8.2 对齐）。
const TOOLS: [&str; 11] = [
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
    "get_skill",
];

pub struct AgentService<W, I, Q, E, S>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    E: EventStore + ?Sized,
    S: SkillRepository + ?Sized,
{
    workspaces: Arc<W>,
    items: Arc<I>,
    questions: Arc<Q>,
    events: Arc<E>,
    /// 用户自定义 Skill 仓储：按用户隔离，bootstrap 与内置合并下发。
    skills_repo: Arc<S>,
    /// 系统内置 Skill 目录：开发者维护，bootstrap 全量下发。
    skills: Vec<Skill>,
}

impl<W, I, Q, E, S> AgentService<W, I, Q, E, S>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    E: EventStore + ?Sized,
    S: SkillRepository + ?Sized,
{
    pub fn new(
        workspaces: Arc<W>,
        items: Arc<I>,
        questions: Arc<Q>,
        events: Arc<E>,
        skills_repo: Arc<S>,
        skills: Vec<Skill>,
    ) -> Self {
        Self {
            workspaces,
            items,
            questions,
            events,
            skills_repo,
            skills,
        }
    }

    /// AgentBootstrap：能力下发。提示词基于用户首个空间的考试目标定制；
    /// 尚无空间时给出引导文案，能力本身始终可用。合并 Skill 目录全量下发
    /// （含脚本）：系统内置 + 用户自定义（同名用户自定义覆盖内置），
    /// Agent 首次接入即自动下载安装。
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
                 请先 bootstrap 获取能力包（含 Skill 目录），把 skills 全部安装后，\
                 再按用户指令生成目录、笔记与习题。"
            ),
            _ => "你是学伴备考助手。用户尚未设置考试目标：请先引导创建备考空间 \
                  （create_workspace），再按指令生成内容。"
                .to_owned(),
        };
        Ok(AgentCapability {
            assistant: "xueban-study-assistant".to_owned(),
            prompt,
            tools: TOOLS.iter().map(|t| (*t).to_owned()).collect(),
            skills: self.merged_skills(user_id).await?,
            version: CAPABILITY_VERSION,
        })
    }

    /// 合并 Skill 目录：内置在前（保持原排序），用户自定义按 id 追加，
    /// 同名用户自定义替换内置的定义（覆盖语义）。
    async fn merged_skills(&self, user_id: i64) -> Result<Vec<Skill>> {
        let mut merged = self.skills.clone();
        for s in self.skills_repo.list_by_user(user_id).await? {
            let definition = Skill {
                name: s.name,
                description: s.description,
                script: s.script,
            };
            match merged.iter_mut().find(|b| b.name == definition.name) {
                Some(existing) => *existing = definition,
                None => merged.push(definition),
            }
        }
        Ok(merged)
    }

    /// GetSkill：按名拉取单个 skill 完整内容（Agent 更新/重新安装用），
    /// 用户自定义优先，无则查系统内置。
    pub async fn get_skill(&self, user_id: i64, name: &str) -> Result<Skill> {
        if let Some(s) = self
            .skills_repo
            .find_by_name_and_user(name, user_id)
            .await?
        {
            return Ok(Skill {
                name: s.name,
                description: s.description,
                script: s.script,
            });
        }
        self.skills
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or_else(|| Error::NotFound("skill 不存在".to_owned()))
    }

    /// CreateSkill：保存用户自定义 skill。名称与介绍 trim 后非空；重名由
    /// 仓储唯一约束拒绝（Conflict 透传）。script 可空（纯说明型 skill）。
    pub async fn create_skill(
        &self,
        user_id: i64,
        name: String,
        description: String,
        script: Option<String>,
    ) -> Result<UserSkill> {
        let name = name.trim().to_owned();
        let description = description.trim().to_owned();
        if name.is_empty() || description.is_empty() {
            return Err(Error::Invalid("skill 名称与介绍不能为空".to_owned()));
        }
        let now = Utc::now();
        let skill = UserSkill {
            id: 0,
            user_id,
            name,
            description,
            script,
            created_at: now,
        };
        let id = self.skills_repo.insert(&skill).await?;
        Ok(UserSkill { id, ..skill })
    }

    /// ListSkills：用户自定义 skill 清单（客户端管理用，不含内置）。
    pub async fn list_skills(&self, user_id: i64) -> Result<Vec<UserSkill>> {
        self.skills_repo.list_by_user(user_id).await
    }

    /// DeleteSkill：删除用户自定义 skill（带归属校验），未命中返回 NotFound。
    pub async fn delete_skill(&self, user_id: i64, id: i64) -> Result<()> {
        if !self.skills_repo.delete(id, user_id).await? {
            return Err(Error::NotFound("skill 不存在".to_owned()));
        }
        Ok(())
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
        InMemorySkillRepository, InMemoryWorkspaceRepository, insert_item,
    };
    use domain::space::ItemKind;

    struct Ctx {
        svc: AgentService<
            InMemoryWorkspaceRepository,
            InMemoryItemRepository,
            InMemoryQuestionRepository,
            InMemoryEventStore,
            InMemorySkillRepository,
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
            Arc::new(InMemorySkillRepository::default()),
            Vec::new(),
        );
        Ctx {
            svc,
            ws_id,
            item_id,
            item_repo,
        }
    }

    fn catalog() -> Vec<Skill> {
        vec![
            Skill {
                name: "链接转笔记".into(),
                description: "把链接内容整理成笔记".into(),
                script: Some("步骤1：解析链接\n步骤2：生成框架笔记".into()),
            },
            Skill {
                name: "习题生成".into(),
                description: "基于笔记生成习题".into(),
                script: None,
            },
        ]
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
        assert_eq!(cap.tools.len(), 11);
        assert!(cap.tools.contains(&"save_questions".to_owned()));
        assert!(cap.tools.contains(&"get_skill".to_owned()));
        // 无内置 skill 时清单为空。
        assert!(cap.skills.is_empty());
    }

    #[tokio::test]
    async fn bootstrap_without_workspace_guides_creation() {
        let ws_repo = Arc::new(InMemoryWorkspaceRepository::default());
        let svc = AgentService::new(
            ws_repo,
            Arc::new(InMemoryItemRepository::default()),
            Arc::new(InMemoryQuestionRepository::default()),
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemorySkillRepository::default()),
            Vec::new(),
        );
        let cap = svc.bootstrap(2).await.unwrap();
        assert!(cap.prompt.contains("create_workspace"));
        assert!(cap.skills.is_empty());
    }

    #[tokio::test]
    async fn bootstrap_installs_full_skill_catalog() {
        let c = ctx().await;
        let svc = AgentService::new(
            c.svc.workspaces.clone(),
            c.item_repo.clone(),
            Arc::new(InMemoryQuestionRepository::default()),
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemorySkillRepository::default()),
            catalog(),
        );
        let cap = svc.bootstrap(1).await.unwrap();
        // 全量下发：含脚本，Agent 首次接入即可安装。
        assert_eq!(cap.skills, catalog());
        assert_eq!(cap.skills.len(), 2);
        assert_eq!(
            cap.skills[0].script.as_deref(),
            Some("步骤1：解析链接\n步骤2：生成框架笔记")
        );
        // 目录全局共享：任何用户接入拿到同一份。
        let other = svc.bootstrap(2).await.unwrap();
        assert_eq!(other.skills, catalog());
    }

    #[tokio::test]
    async fn bootstrap_merges_user_skills_over_builtin() {
        let c = ctx().await;
        let svc = AgentService::new(
            c.svc.workspaces.clone(),
            c.item_repo.clone(),
            Arc::new(InMemoryQuestionRepository::default()),
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemorySkillRepository::default()),
            catalog(),
        );
        // 用户自定义一个全新 skill + 一个与内置同名的覆盖 skill。
        let _ = svc
            .create_skill(
                1,
                "错题复盘".into(),
                "分析错题原因".into(),
                Some("脚本A".into()),
            )
            .await
            .unwrap();
        let _ = svc
            .create_skill(
                1,
                "链接转笔记".into(),
                "用户版介绍".into(),
                Some("用户版脚本".into()),
            )
            .await
            .unwrap();
        let cap = svc.bootstrap(1).await.unwrap();
        assert_eq!(cap.skills.len(), 3);
        // 同名覆盖：链接转笔记取用户版内容。
        let overwritten = cap.skills.iter().find(|s| s.name == "链接转笔记").unwrap();
        assert_eq!(overwritten.description, "用户版介绍");
        assert_eq!(overwritten.script.as_deref(), Some("用户版脚本"));
        // 全新 skill 追加在目录尾部。
        assert_eq!(cap.skills[2].name, "错题复盘");
        assert_eq!(cap.skills[2].script.as_deref(), Some("脚本A"));
        // 目录按用户隔离：他人接入只拿内置。
        let other = svc.bootstrap(2).await.unwrap();
        assert_eq!(other.skills, catalog());
    }

    #[tokio::test]
    async fn get_skill_prefers_user_skill_and_isolates_users() {
        let c = ctx().await;
        let svc = AgentService::new(
            c.svc.workspaces.clone(),
            c.item_repo.clone(),
            Arc::new(InMemoryQuestionRepository::default()),
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemorySkillRepository::default()),
            catalog(),
        );
        let _ = svc
            .create_skill(
                1,
                "链接转笔记".into(),
                "用户版介绍".into(),
                Some("用户版脚本".into()),
            )
            .await
            .unwrap();
        // 用户优先：同名取用户自定义。
        let s = svc.get_skill(1, "链接转笔记").await.unwrap();
        assert_eq!(s.script.as_deref(), Some("用户版脚本"));
        // 无同名时回退内置。
        let s = svc.get_skill(1, "习题生成").await.unwrap();
        assert_eq!(s.description, "基于笔记生成习题");
        // 跨用户：他人查不到用户自定义，回退内置。
        let s = svc.get_skill(2, "链接转笔记").await.unwrap();
        assert_eq!(
            s.script.as_deref(),
            Some("步骤1：解析链接\n步骤2：生成框架笔记")
        );
        assert!(matches!(
            svc.get_skill(1, "不存在").await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn create_skill_validates_and_rejects_duplicate() {
        let c = ctx().await;
        // 空名 / 空介绍 → Invalid。
        assert!(matches!(
            c.svc
                .create_skill(1, "  ".into(), "介绍".into(), None)
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            c.svc
                .create_skill(1, "名字".into(), "  ".into(), None)
                .await,
            Err(Error::Invalid(_))
        ));
        // 正常创建：trim 名称。
        let s = c
            .svc
            .create_skill(1, " 错题复盘 ".into(), "介绍".into(), Some("脚本".into()))
            .await
            .unwrap();
        assert_eq!(s.name, "错题复盘");
        // 同用户重名 → Conflict。
        assert!(matches!(
            c.svc
                .create_skill(1, "错题复盘".into(), "介绍".into(), None)
                .await,
            Err(Error::Conflict(_))
        ));
        // 他人同名不冲突。
        assert!(
            c.svc
                .create_skill(2, "错题复盘".into(), "介绍".into(), None)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn list_and_delete_skills_isolate_users() {
        let c = ctx().await;
        let id = c
            .svc
            .create_skill(1, "错题复盘".into(), "介绍".into(), None)
            .await
            .unwrap()
            .id;
        let _ = c
            .svc
            .create_skill(1, "链接整理".into(), "介绍".into(), None)
            .await
            .unwrap();
        let _ = c
            .svc
            .create_skill(2, "他人 skill".into(), "介绍".into(), None)
            .await
            .unwrap();
        // 清单按用户隔离且 id 升序。
        let mine = c.svc.list_skills(1).await.unwrap();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].name, "错题复盘");
        assert_eq!(mine[1].name, "链接整理");
        assert_eq!(c.svc.list_skills(2).await.unwrap().len(), 1);
        // 删除：成功 / 他人 id → NotFound / 重复删 → NotFound。
        c.svc.delete_skill(1, id).await.unwrap();
        assert!(matches!(
            c.svc.delete_skill(2, id).await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            c.svc.delete_skill(1, id).await,
            Err(Error::NotFound(_))
        ));
        assert_eq!(c.svc.list_skills(1).await.unwrap().len(), 1);
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
