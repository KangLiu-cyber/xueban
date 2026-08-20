//! 学习空间用例：ManageExamGoal / BrowseTree / ReadNote / Annotate。
//!
//! Agent 内容写入（create/write item）复用同一组方法，`created_by` 由调用方
//! 按驱动入口给定（MCP 侧固定 Agent），并追加 agent_write 事件供客户端溯源。

use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::{AnnotationRepository, EventStore, ItemRepository, WorkspaceRepository};
use domain::space::{
    Annotation, AnnotationAuthor, Creator, Item, ItemKind, ItemNode, Workspace, assert_no_cycle,
};
use serde::Serialize;

/// 单 item 正文上限：512KB（架构文档 §10 输入校验）。
pub const MAX_ITEM_CONTENT_BYTES: usize = 512 * 1024;

pub struct SpaceService<W, I, A, E>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    A: AnnotationRepository + ?Sized,
    E: EventStore + ?Sized,
{
    workspaces: Arc<W>,
    items: Arc<I>,
    annotations: Arc<A>,
    events: Arc<E>,
}

impl<W, I, A, E> SpaceService<W, I, A, E>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    A: AnnotationRepository + ?Sized,
    E: EventStore + ?Sized,
{
    pub fn new(workspaces: Arc<W>, items: Arc<I>, annotations: Arc<A>, events: Arc<E>) -> Self {
        Self {
            workspaces,
            items,
            annotations,
            events,
        }
    }

    /// ManageExamGoal（创建空间）。
    pub async fn create_workspace(
        &self,
        user_id: i64,
        name: String,
        exam_goal: String,
        exam_date: Option<NaiveDate>,
    ) -> Result<Workspace> {
        let ws = Workspace {
            id: 0,
            user_id,
            name,
            exam_goal,
            exam_date,
            created_at: Utc::now(),
        };
        let id = self.workspaces.insert(&ws).await?;
        let mut ws = ws;
        ws.id = id;
        Ok(ws)
    }

    /// ManageExamGoal（更新目标/日期），空间不存在或不属于该用户时报 NotFound。
    pub async fn update_workspace(
        &self,
        user_id: i64,
        id: i64,
        name: String,
        exam_goal: String,
        exam_date: Option<NaiveDate>,
    ) -> Result<Workspace> {
        let mut ws = self
            .workspaces
            .find_by_id_and_user(id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("学习空间不存在".to_owned()))?;
        ws.set_goal(name, exam_goal, exam_date);
        self.workspaces.update(&ws).await?;
        Ok(ws)
    }

    /// 用户的全部空间（BrowseTree 的空间列表）。
    pub async fn list_workspaces(&self, user_id: i64) -> Result<Vec<Workspace>> {
        self.workspaces.list_by_user(user_id).await
    }

    /// 删除空间（先做归属校验，未命中报 NotFound；级联由存储层承担）。
    pub async fn delete_workspace(&self, user_id: i64, id: i64) -> Result<()> {
        self.require_workspace(id, user_id).await?;
        let hit = self.workspaces.delete(id, user_id).await?;
        if hit {
            Ok(())
        } else {
            Err(Error::NotFound("学习空间不存在".to_owned()))
        }
    }

    /// 空间归属校验：不存在或不属于该用户时报 NotFound。
    pub async fn require_workspace(&self, id: i64, user_id: i64) -> Result<Workspace> {
        self.workspaces
            .find_by_id_and_user(id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("学习空间不存在".to_owned()))
    }

    /// BrowseTree：完整内容树。
    pub async fn tree(&self, user_id: i64, workspace_id: i64) -> Result<Vec<ItemNode>> {
        self.require_workspace(workspace_id, user_id).await?;
        self.items.list_tree(workspace_id, user_id).await
    }

    /// ReadNote：按归属取节点（目录或笔记均可读）。
    /// item 不存 user_id，归属经所属空间校验。
    pub async fn read_item(&self, user_id: i64, item_id: i64) -> Result<Item> {
        let item = self
            .items
            .find_by_id(item_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("内容不存在".to_owned()))?;
        self.require_workspace(item.workspace_id, user_id).await?;
        Ok(item)
    }

    /// 客户端写入目录/笔记（created_by = user，不落事件）。
    pub async fn create_item(
        &self,
        user_id: i64,
        workspace_id: i64,
        parent_id: Option<i64>,
        kind: ItemKind,
        name: String,
        content: Option<String>,
    ) -> Result<Item> {
        self.create_item_inner(
            user_id,
            workspace_id,
            parent_id,
            NewItem {
                kind,
                name,
                content,
                created_by: Creator::User,
            },
        )
        .await
    }

    /// Agent 写入目录/笔记（created_by = ai，落 agent_write 事件）。
    pub async fn agent_create_item(
        &self,
        user_id: i64,
        workspace_id: i64,
        parent_id: Option<i64>,
        kind: ItemKind,
        name: String,
        content: Option<String>,
    ) -> Result<Item> {
        let item = self
            .create_item_inner(
                user_id,
                workspace_id,
                parent_id,
                NewItem {
                    kind,
                    name,
                    content,
                    created_by: Creator::Agent,
                },
            )
            .await?;
        self.log_agent_write(user_id, workspace_id, Some(item.id))
            .await?;
        Ok(item)
    }

    async fn create_item_inner(
        &self,
        user_id: i64,
        workspace_id: i64,
        parent_id: Option<i64>,
        input: NewItem,
    ) -> Result<Item> {
        let NewItem {
            kind,
            name,
            content,
            created_by,
        } = input;
        self.require_workspace(workspace_id, user_id).await?;
        if let Some(parent) = parent_id {
            let parent_item = self.read_item(user_id, parent).await?;
            if !parent_item.is_dir() {
                return Err(Error::Invalid("父节点必须是目录".to_owned()));
            }
            // 防环不变式挂在写入口：新建节点 id 尚未分配，以占位 0 调用
            //（0 永不与真实节点冲突），健康树上必然通过；树若损坏则在此终止。
            let chain = self.items.ancestors(parent, user_id).await?;
            assert_no_cycle(Some(parent), 0, &chain)?;
        }
        if let Some(content) = &content {
            Self::check_content_size(content)?;
        }
        let now = Utc::now();
        let item = Item {
            id: 0,
            workspace_id,
            parent_id,
            kind,
            name,
            content,
            created_by,
            created_at: now,
            updated_at: now,
        };
        let id = self.items.insert(&item).await?;
        let mut item = item;
        item.id = id;
        Ok(item)
    }

    /// 更新笔记正文（用户编辑；目录无正文）。
    pub async fn write_item(&self, user_id: i64, item_id: i64, content: String) -> Result<Item> {
        let mut item = self.read_item(user_id, item_id).await?;
        if item.is_dir() {
            return Err(Error::Invalid("目录没有正文".to_owned()));
        }
        Self::check_content_size(&content)?;
        item.content = Some(content);
        item.updated_at = Utc::now();
        self.items.update(&item, user_id).await?;
        Ok(item)
    }

    /// Agent 更新笔记正文（落 agent_write 事件）。
    pub async fn agent_write_item(
        &self,
        user_id: i64,
        item_id: i64,
        content: String,
    ) -> Result<Item> {
        let item = self.write_item(user_id, item_id, content).await?;
        self.log_agent_write(user_id, item.workspace_id, Some(item.id))
            .await?;
        Ok(item)
    }

    /// Annotate：在笔记上追加批注，落 annotate 事件。
    /// Agent 批注（author = Ai）同时追加 agent_write 事件（§8.2 强制规则：
    /// Agent 写入全部记入 events，客户端可展示"由 AI 生成"来源标注）。
    pub async fn annotate(
        &self,
        user_id: i64,
        item_id: i64,
        anchor: String,
        text: String,
        author: AnnotationAuthor,
    ) -> Result<Annotation> {
        let item = self.read_item(user_id, item_id).await?;
        if !item.is_note() {
            return Err(Error::Invalid("批注只能加在笔记上".to_owned()));
        }
        let mut ann = Annotation {
            id: 0,
            item_id,
            user_id,
            author,
            anchor,
            text,
            created_at: Utc::now(),
        };
        let id = self.annotations.insert(&ann).await?;
        ann.id = id;
        self.events
            .append(&Event {
                id: 0,
                user_id,
                workspace_id: Some(item.workspace_id),
                item_id: Some(item_id),
                action: EventAction::Annotate,
                payload: Some(
                    serde_json::to_string(&AnnotatePayload {
                        annotation_id: id,
                        author,
                    })
                    .unwrap(),
                ),
                created_at: Utc::now(),
            })
            .await?;
        if author == AnnotationAuthor::Ai {
            self.log_agent_write(user_id, item.workspace_id, Some(item_id))
                .await?;
        }
        Ok(ann)
    }

    /// 编辑批注文本：仅「我的批注」可编辑（AI 批注只读），归属校验由仓储完成。
    pub async fn edit_annotation(
        &self,
        user_id: i64,
        annotation_id: i64,
        text: String,
    ) -> Result<Annotation> {
        let mut ann = self
            .annotations
            .find_by_id(annotation_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("批注不存在".to_owned()))?;
        if ann.author != AnnotationAuthor::User {
            return Err(Error::Invalid("AI 批注不可编辑".to_owned()));
        }
        ann.text = text;
        let hit = self.annotations.update(&ann, user_id).await?;
        if !hit {
            return Err(Error::NotFound("批注不存在".to_owned()));
        }
        Ok(ann)
    }

    /// 删除批注（归属校验由仓储完成，未命中报 NotFound）。
    pub async fn delete_annotation(&self, user_id: i64, annotation_id: i64) -> Result<()> {
        let hit = self.annotations.delete(annotation_id, user_id).await?;
        if hit {
            Ok(())
        } else {
            Err(Error::NotFound("批注不存在".to_owned()))
        }
    }

    /// 删除目录/笔记：先经 ReadNote 归属校验，再删除。
    /// 级联行为（子树/批注/归属题目）由存储层承担（SQL ON DELETE CASCADE）。
    pub async fn delete_item(&self, user_id: i64, item_id: i64) -> Result<()> {
        self.read_item(user_id, item_id).await?;
        let hit = self.items.delete(item_id, user_id).await?;
        if hit {
            Ok(())
        } else {
            Err(Error::NotFound("内容不存在".to_owned()))
        }
    }

    /// 列某笔记的批注（先校验笔记归属）。
    pub async fn list_annotations(&self, user_id: i64, item_id: i64) -> Result<Vec<Annotation>> {
        self.read_item(user_id, item_id).await?;
        self.annotations.list_by_item(item_id, user_id).await
    }

    fn check_content_size(content: &str) -> Result<()> {
        if content.len() > MAX_ITEM_CONTENT_BYTES {
            return Err(Error::Invalid("笔记正文超过 512KB 上限".to_owned()));
        }
        Ok(())
    }

    async fn log_agent_write(
        &self,
        user_id: i64,
        workspace_id: i64,
        item_id: Option<i64>,
    ) -> Result<()> {
        self.events
            .append(&Event {
                id: 0,
                user_id,
                workspace_id: Some(workspace_id),
                item_id,
                action: EventAction::AgentWrite,
                payload: Some(
                    serde_json::to_string(&AgentWritePayload {
                        workspace_id,
                        item_id,
                    })
                    .unwrap(),
                ),
                created_at: Utc::now(),
            })
            .await?;
        Ok(())
    }
}

/// create_item_inner 入参分组（避免过长的参数列表）。
struct NewItem {
    kind: ItemKind,
    name: String,
    content: Option<String>,
    created_by: Creator,
}

#[derive(Serialize)]
struct AnnotatePayload {
    annotation_id: i64,
    author: AnnotationAuthor,
}

#[derive(Serialize)]
struct AgentWritePayload {
    workspace_id: i64,
    item_id: Option<i64>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryAnnotationRepository, InMemoryEventStore, InMemoryItemRepository,
        InMemoryWorkspaceRepository,
    };

    fn svc() -> SpaceService<
        InMemoryWorkspaceRepository,
        InMemoryItemRepository,
        InMemoryAnnotationRepository,
        InMemoryEventStore,
    > {
        SpaceService::new(
            Arc::new(InMemoryWorkspaceRepository::default()),
            Arc::new(InMemoryItemRepository::default()),
            Arc::new(InMemoryAnnotationRepository::default()),
            Arc::new(InMemoryEventStore::default()),
        )
    }

    #[tokio::test]
    async fn create_and_update_workspace() {
        let s = svc();
        let ws = s
            .create_workspace(1, "我的备考".into(), "软考架构师".into(), None)
            .await
            .unwrap();
        let updated = s
            .update_workspace(1, ws.id, "考研".into(), "总分 400".into(), None)
            .await
            .unwrap();
        assert_eq!(updated.name, "考研");
        assert_eq!(updated.exam_goal, "总分 400");
        assert_eq!(s.list_workspaces(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn update_workspace_isolates_users() {
        let s = svc();
        let ws = s
            .create_workspace(1, "a".into(), "g".into(), None)
            .await
            .unwrap();
        assert!(
            s.update_workspace(2, ws.id, "x".into(), "y".into(), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn tree_and_read_item() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let dir = s
            .create_item(1, ws.id, None, ItemKind::Dir, "第1集".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                Some(dir.id),
                ItemKind::Note,
                "软件架构概念".into(),
                Some("# 标题".into()),
            )
            .await
            .unwrap();
        let tree = s.tree(1, ws.id).await.unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].item.id, note.id);
        // 跨用户不可见。
        assert!(s.tree(2, ws.id).await.is_err());
    }

    #[tokio::test]
    async fn create_item_rejects_non_dir_parent() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "笔记".into(),
                Some("x".into()),
            )
            .await
            .unwrap();
        assert!(
            s.create_item(
                1,
                ws.id,
                Some(note.id),
                ItemKind::Note,
                "子".into(),
                Some("y".into())
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn create_item_terminates_on_corrupted_cycle_tree() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        // 直接经仓储构造损坏树 A↔B（互相为父）——应用层不产生此状态，仅测试守卫。
        let make = |parent_id, name: &str| Item {
            id: 0,
            workspace_id: ws.id,
            parent_id,
            kind: ItemKind::Dir,
            name: name.into(),
            content: None,
            created_by: Creator::User,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let a = s.items.insert(&make(None, "A")).await.unwrap();
        let b = s.items.insert(&make(Some(a), "B")).await.unwrap();
        let mut a_item = make(Some(b), "A");
        a_item.id = a;
        s.items.update(&a_item, 1).await.unwrap();

        // 在损坏环节点下创建：占位 0 永不构成环，创建成功；ancestors 的
        // visited 守卫保证遍历终止（无该守卫则此处无限循环挂起）。
        let created = s
            .create_item(
                1,
                ws.id,
                Some(a),
                ItemKind::Note,
                "C".into(),
                Some("x".into()),
            )
            .await
            .expect("损坏树上创建不应失败");
        assert_eq!(created.name, "C");
        assert!(created.id > 0);
    }

    #[tokio::test]
    async fn create_and_write_reject_oversized_content() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let huge = "x".repeat(MAX_ITEM_CONTENT_BYTES + 1);
        assert!(matches!(
            s.create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "大笔记".into(),
                Some(huge.clone())
            )
            .await,
            Err(Error::Invalid(_))
        ));
        // 正好 512KB 可通过。
        let ok = "x".repeat(MAX_ITEM_CONTENT_BYTES);
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "边界".into(),
                Some(ok.clone()),
            )
            .await
            .unwrap();
        assert_eq!(note.content.as_deref(), Some(ok.as_str()));
        assert!(matches!(
            s.write_item(1, note.id, huge).await,
            Err(Error::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn write_item_updates_content_and_rejects_dir() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "笔记".into(),
                Some("old".into()),
            )
            .await
            .unwrap();
        let dir = s
            .create_item(1, ws.id, None, ItemKind::Dir, "目录".into(), None)
            .await
            .unwrap();
        let updated = s.write_item(1, note.id, "new".into()).await.unwrap();
        assert_eq!(updated.content.as_deref(), Some("new"));
        assert!(s.write_item(1, dir.id, "x".into()).await.is_err());
    }

    #[tokio::test]
    async fn agent_write_logs_agent_write_event() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let note = s
            .agent_create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "AI 笔记".into(),
                Some("# 内容".into()),
            )
            .await
            .unwrap();
        assert_eq!(note.created_by, Creator::Agent);
        let events = s.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::AgentWrite);
        assert_eq!(events[0].item_id, Some(note.id));
        // 用户写入不落事件。
        s.create_item(1, ws.id, None, ItemKind::Dir, "用户目录".into(), None)
            .await
            .unwrap();
        assert_eq!(s.events.list_by_user(1, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn annotate_and_delete() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "笔记".into(),
                Some("正文".into()),
            )
            .await
            .unwrap();
        let ann = s
            .annotate(
                1,
                note.id,
                "正文".into(),
                "考点".into(),
                AnnotationAuthor::User,
            )
            .await
            .unwrap();
        assert_eq!(s.list_annotations(1, note.id).await.unwrap().len(), 1);
        // 用户批注只落 annotate 事件。
        let events = s.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::Annotate);

        // Agent 批注同时追加 agent_write 事件（§8.2 来源标注）；列表时间升序。
        s.annotate(
            1,
            note.id,
            "正文".into(),
            "AI 考点".into(),
            AnnotationAuthor::Ai,
        )
        .await
        .unwrap();
        let events = s.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].action, EventAction::Annotate);
        assert_eq!(events[1].action, EventAction::Annotate);
        assert_eq!(events[2].action, EventAction::AgentWrite);
        assert_eq!(events[2].item_id, Some(note.id));

        s.delete_annotation(1, ann.id).await.unwrap();
        let rest = s.list_annotations(1, note.id).await.unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].author, AnnotationAuthor::Ai);
    }

    #[tokio::test]
    async fn edit_annotation_updates_own_user_annotation_only() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "笔记".into(),
                Some("正文".into()),
            )
            .await
            .unwrap();
        let mine = s
            .annotate(
                1,
                note.id,
                "正文".into(),
                "原内容".into(),
                AnnotationAuthor::User,
            )
            .await
            .unwrap();
        let ai = s
            .annotate(
                1,
                note.id,
                "正文".into(),
                "AI 批注".into(),
                AnnotationAuthor::Ai,
            )
            .await
            .unwrap();

        // 编辑自己的批注成功。
        let edited = s
            .edit_annotation(1, mine.id, "新内容".into())
            .await
            .unwrap();
        assert_eq!(edited.text, "新内容");
        assert_eq!(
            s.list_annotations(1, note.id).await.unwrap()[0].text,
            "新内容"
        );
        // AI 批注不可编辑。
        assert!(s.edit_annotation(1, ai.id, "x".into()).await.is_err());
        // 他人/不存在的批注不可达（NotFound）。
        assert!(s.edit_annotation(2, mine.id, "x".into()).await.is_err());
        assert!(s.edit_annotation(1, 999_999, "x".into()).await.is_err());
    }

    #[tokio::test]
    async fn annotate_isolates_users_and_dirs() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let dir = s
            .create_item(1, ws.id, None, ItemKind::Dir, "目录".into(), None)
            .await
            .unwrap();
        assert!(
            s.annotate(1, dir.id, "a".into(), "b".into(), AnnotationAuthor::User)
                .await
                .is_err()
        );
        // 他人在笔记上加批注不可见、删除不可达。
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "笔记".into(),
                Some("x".into()),
            )
            .await
            .unwrap();
        assert!(s.list_annotations(2, note.id).await.is_err());
    }

    #[tokio::test]
    async fn delete_item_removes_note_and_subtree() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let dir = s
            .create_item(1, ws.id, None, ItemKind::Dir, "第1集".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                Some(dir.id),
                ItemKind::Note,
                "笔记".into(),
                Some("正文".into()),
            )
            .await
            .unwrap();
        let ann = s
            .annotate(
                1,
                note.id,
                "正文".into(),
                "考点".into(),
                AnnotationAuthor::Ai,
            )
            .await
            .unwrap();
        // 删除笔记：笔记与其批注一并消失。
        s.delete_item(1, note.id).await.unwrap();
        assert!(matches!(
            s.read_item(1, note.id).await,
            Err(Error::NotFound(_))
        ));
        assert!(s.list_annotations(1, ann.item_id).await.is_err());
        // 删除目录：整棵子树消失。
        s.delete_item(1, dir.id).await.unwrap();
        assert!(matches!(
            s.read_item(1, dir.id).await,
            Err(Error::NotFound(_))
        ));
        assert!(s.tree(1, ws.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_item_rejects_foreign_and_unknown() {
        let s = svc();
        let ws = s
            .create_workspace(1, "备考".into(), "目标".into(), None)
            .await
            .unwrap();
        let note = s
            .create_item(
                1,
                ws.id,
                None,
                ItemKind::Note,
                "笔记".into(),
                Some("x".into()),
            )
            .await
            .unwrap();
        // 他人不可删。
        assert!(matches!(
            s.delete_item(2, note.id).await,
            Err(Error::NotFound(_))
        ));
        assert!(s.read_item(1, note.id).await.is_ok());
        // 不存在的节点。
        assert!(matches!(
            s.delete_item(1, 999).await,
            Err(Error::NotFound(_))
        ));
    }
}
