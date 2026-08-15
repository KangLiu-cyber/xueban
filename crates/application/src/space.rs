//! 学习空间用例：ManageExamGoal / BrowseTree / ReadNote / Annotate。
//!
//! Agent 内容写入（create/write item）复用同一组方法，`created_by` 由调用方
//! 按驱动入口给定（MCP 侧固定 Agent），并追加 agent_write 事件供客户端溯源。

use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::{AnnotationRepository, EventStore, ItemRepository, WorkspaceRepository};
use domain::space::{Annotation, AnnotationAuthor, Creator, Item, ItemKind, ItemNode, Workspace};
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
            .ok_or_else(|| Error::NotFound("备考空间不存在".to_owned()))?;
        ws.set_goal(name, exam_goal, exam_date);
        self.workspaces.update(&ws).await?;
        Ok(ws)
    }

    /// 用户的全部空间（BrowseTree 的空间列表）。
    pub async fn list_workspaces(&self, user_id: i64) -> Result<Vec<Workspace>> {
        self.workspaces.list_by_user(user_id).await
    }

    /// 空间归属校验：不存在或不属于该用户时报 NotFound。
    pub async fn require_workspace(&self, id: i64, user_id: i64) -> Result<Workspace> {
        self.workspaces
            .find_by_id_and_user(id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("备考空间不存在".to_owned()))
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
        self.items.update(&item).await?;
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
        s.delete_annotation(1, ann.id).await.unwrap();
        assert!(s.list_annotations(1, note.id).await.unwrap().is_empty());
        // 事件已记录。
        let events = s.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events[0].action, EventAction::Annotate);
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
}
