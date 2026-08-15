//! 学习空间输出端口实现：workspaces / items / annotations 三张表。
//!
//! 读路径在 SQL 层强制 user_id 归属（workspaces join），作为隔离第二道防线；
//! 写路径（update/delete）同样在 SQL 层以 user_id 守卫，应用层校验之外再加一道防线。

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use domain::error::{Error, Result};
use domain::ports::{AnnotationRepository, ItemRepository, WorkspaceRepository};
use domain::space::{Annotation, AnnotationAuthor, Creator, Item, ItemKind, ItemNode, Workspace};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::map_sqlx_error;

pub struct PgWorkspaceRepository {
    pool: PgPool,
}

impl PgWorkspaceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkspaceRepository for PgWorkspaceRepository {
    async fn insert(&self, ws: &Workspace) -> Result<i64> {
        let row = sqlx::query(
            "insert into workspaces (user_id, name, exam_goal, exam_date)
             values ($1, $2, $3, $4) returning id",
        )
        .bind(ws.user_id)
        .bind(&ws.name)
        .bind(&ws.exam_goal)
        .bind(ws.exam_date)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn find_by_id_and_user(&self, id: i64, user_id: i64) -> Result<Option<Workspace>> {
        sqlx::query(
            "select id, user_id, name, exam_goal, exam_date, created_at
             from workspaces where id = $1 and user_id = $2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| workspace_from_row(&row))
        .transpose()
    }

    async fn list_by_user(&self, user_id: i64) -> Result<Vec<Workspace>> {
        let rows = sqlx::query(
            "select id, user_id, name, exam_goal, exam_date, created_at
             from workspaces where user_id = $1 order by id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(workspace_from_row).collect()
    }

    async fn update(&self, ws: &Workspace) -> Result<()> {
        sqlx::query(
            "update workspaces set name = $2, exam_goal = $3, exam_date = $4
             where id = $1 and user_id = $5",
        )
        .bind(ws.id)
        .bind(&ws.name)
        .bind(&ws.exam_goal)
        .bind(ws.exam_date)
        .bind(ws.user_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }
}

fn workspace_from_row(row: &PgRow) -> Result<Workspace> {
    Ok(Workspace {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        user_id: row.try_get::<i64, _>("user_id").map_err(map_sqlx_error)?,
        name: row.try_get::<String, _>("name").map_err(map_sqlx_error)?,
        exam_goal: row
            .try_get::<String, _>("exam_goal")
            .map_err(map_sqlx_error)?,
        exam_date: row
            .try_get::<Option<NaiveDate>, _>("exam_date")
            .map_err(map_sqlx_error)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
    })
}

pub struct PgItemRepository {
    pool: PgPool,
}

impl PgItemRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ItemRepository for PgItemRepository {
    async fn insert(&self, item: &Item) -> Result<i64> {
        let row = sqlx::query(
            "insert into items (workspace_id, parent_id, kind, name, content, created_by)
             values ($1, $2, $3, $4, $5, $6) returning id",
        )
        .bind(item.workspace_id)
        .bind(item.parent_id)
        .bind(item.kind.as_str())
        .bind(&item.name)
        .bind(&item.content)
        .bind(creator_str(item.created_by))
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    // 写路径守卫：join workspaces 限定归属（第二道防线），应用层校验之外再加一道。
    async fn update(&self, item: &Item, user_id: i64) -> Result<()> {
        sqlx::query(
            "update items i set parent_id = $2, kind = $3, name = $4, content = $5, updated_at = $6
             from workspaces w
             where i.id = $1 and w.id = i.workspace_id and w.user_id = $7",
        )
        .bind(item.id)
        .bind(item.parent_id)
        .bind(item.kind.as_str())
        .bind(&item.name)
        .bind(&item.content)
        .bind(item.updated_at)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn find_by_id(&self, id: i64, user_id: i64) -> Result<Option<Item>> {
        sqlx::query(
            "select i.id, i.workspace_id, i.parent_id, i.kind, i.name, i.content,
                    i.created_by, i.created_at, i.updated_at
             from items i
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where i.id = $1",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| item_from_row(&row))
        .transpose()
    }

    // 归属守卫：join workspaces 限定（第二道防线）。
    async fn list_children(
        &self,
        workspace_id: i64,
        user_id: i64,
        parent_id: Option<i64>,
    ) -> Result<Vec<Item>> {
        let rows = sqlx::query(
            "select i.id, i.workspace_id, i.parent_id, i.kind, i.name, i.content,
                    i.created_by, i.created_at, i.updated_at
             from items i
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where i.workspace_id = $1 and i.parent_id is not distinct from $3
             order by i.id",
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(item_from_row).collect()
    }

    async fn list_tree(&self, workspace_id: i64, user_id: i64) -> Result<Vec<ItemNode>> {
        let rows = sqlx::query(
            "with recursive tree as (
               select i.id, i.workspace_id, i.parent_id, i.kind, i.name, i.content,
                      i.created_by, i.created_at, i.updated_at
               from items i
               join workspaces w on w.id = i.workspace_id and w.user_id = $2
               where i.workspace_id = $1 and i.parent_id is null
               union all
               select i.id, i.workspace_id, i.parent_id, i.kind, i.name, i.content,
                      i.created_by, i.created_at, i.updated_at
               from items i
               join tree t on i.parent_id = t.id
             )
             select id, workspace_id, parent_id, kind, name, content,
                    created_by, created_at, updated_at
             from tree order by id",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let items = rows.iter().map(item_from_row).collect::<Result<Vec<_>>>()?;
        Ok(assemble_tree(items))
    }

    // CTE 产出 [自身, 父, 祖父, ...]，反转成 根→自身（与 inmem 语义一致）。
    // 归属守卫：anchor 行 join workspaces 限定 user（第二道防线）。
    async fn ancestors(&self, item_id: i64, user_id: i64) -> Result<Vec<i64>> {
        let rows = sqlx::query(
            "with recursive chain as (
               select i.id, i.parent_id from items i
               join workspaces w on w.id = i.workspace_id and w.user_id = $2
               where i.id = $1
               union all
               select i.id, i.parent_id from items i join chain c on i.id = c.parent_id
             )
             select id from chain",
        )
        .bind(item_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let mut chain: Vec<i64> = rows
            .iter()
            .map(|row| row.try_get::<i64, _>("id").map_err(map_sqlx_error))
            .collect::<Result<Vec<_>>>()?;
        chain.reverse();
        Ok(chain)
    }

    // 删除节点：SQL 层 join workspaces 限定归属（第二道防线），
    // 级联（子树/批注/归属题目）由 0002 迁移的 ON DELETE CASCADE 承担。
    async fn delete(&self, id: i64, user_id: i64) -> Result<bool> {
        let result = sqlx::query(
            "delete from items i using workspaces w
             where i.id = $1 and w.id = i.workspace_id and w.user_id = $2",
        )
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }
}

fn item_from_row(row: &PgRow) -> Result<Item> {
    let kind: String = row.try_get("kind").map_err(map_sqlx_error)?;
    let created_by: String = row.try_get("created_by").map_err(map_sqlx_error)?;
    Ok(Item {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        workspace_id: row
            .try_get::<i64, _>("workspace_id")
            .map_err(map_sqlx_error)?,
        parent_id: row
            .try_get::<Option<i64>, _>("parent_id")
            .map_err(map_sqlx_error)?,
        kind: kind_from_str(&kind)?,
        name: row.try_get::<String, _>("name").map_err(map_sqlx_error)?,
        content: row
            .try_get::<Option<String>, _>("content")
            .map_err(map_sqlx_error)?,
        created_by: creator_from_str(&created_by)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
        updated_at: row
            .try_get::<DateTime<Utc>, _>("updated_at")
            .map_err(map_sqlx_error)?,
    })
}

fn kind_from_str(s: &str) -> Result<ItemKind> {
    match s {
        "dir" => Ok(ItemKind::Dir),
        "note" => Ok(ItemKind::Note),
        other => Err(Error::Storage(format!("未知节点类型: {other}"))),
    }
}

fn creator_str(c: Creator) -> &'static str {
    match c {
        Creator::Agent => "agent",
        Creator::User => "user",
    }
}

fn creator_from_str(s: &str) -> Result<Creator> {
    match s {
        "agent" => Ok(Creator::Agent),
        "user" => Ok(Creator::User),
        other => Err(Error::Storage(format!("未知创建来源: {other}"))),
    }
}

/// 按 id 升序组装为树（与 inmem 语义一致：子节点按 id 排序）。
fn assemble_tree(items: Vec<Item>) -> Vec<ItemNode> {
    fn build(parent_id: Option<i64>, items: &[Item]) -> Vec<ItemNode> {
        let mut children: Vec<ItemNode> = items
            .iter()
            .filter(|i| i.parent_id == parent_id)
            .map(|i| ItemNode {
                item: i.clone(),
                children: build(Some(i.id), items),
            })
            .collect();
        children.sort_by_key(|n| n.item.id);
        children
    }
    build(None, &items)
}

pub struct PgAnnotationRepository {
    pool: PgPool,
}

impl PgAnnotationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AnnotationRepository for PgAnnotationRepository {
    async fn insert(&self, ann: &Annotation) -> Result<i64> {
        let row = sqlx::query(
            "insert into annotations (item_id, user_id, author, anchor, text)
             values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(ann.item_id)
        .bind(ann.user_id)
        .bind(ann.author.as_str())
        .bind(&ann.anchor)
        .bind(&ann.text)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn find_by_id(&self, id: i64, user_id: i64) -> Result<Option<Annotation>> {
        sqlx::query(
            "select a.id, a.item_id, a.user_id, a.author, a.anchor, a.text, a.created_at
             from annotations a
             join items i on i.id = a.item_id
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where a.id = $1",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| annotation_from_row(&row))
        .transpose()
    }

    async fn list_by_item(&self, item_id: i64, user_id: i64) -> Result<Vec<Annotation>> {
        // join items → workspaces 限定归属：笔记属于 user，跨用户读的第二道防线。
        let rows = sqlx::query(
            "select a.id, a.item_id, a.user_id, a.author, a.anchor, a.text, a.created_at
             from annotations a
             join items i on i.id = a.item_id
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where a.item_id = $1 order by a.id",
        )
        .bind(item_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(annotation_from_row).collect()
    }

    // 写路径守卫：user_id 限定归属（第二道防线）。
    async fn update(&self, ann: &Annotation, user_id: i64) -> Result<bool> {
        let result = sqlx::query("update annotations set text = $3 where id = $1 and user_id = $2")
            .bind(ann.id)
            .bind(user_id)
            .bind(&ann.text)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete(&self, id: i64, user_id: i64) -> Result<bool> {
        let result = sqlx::query("delete from annotations where id = $1 and user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }
}

fn annotation_from_row(row: &PgRow) -> Result<Annotation> {
    let author: String = row.try_get("author").map_err(map_sqlx_error)?;
    Ok(Annotation {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        item_id: row.try_get::<i64, _>("item_id").map_err(map_sqlx_error)?,
        user_id: row.try_get::<i64, _>("user_id").map_err(map_sqlx_error)?,
        author: author_from_str(&author)?,
        anchor: row.try_get::<String, _>("anchor").map_err(map_sqlx_error)?,
        text: row.try_get::<String, _>("text").map_err(map_sqlx_error)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
    })
}

fn author_from_str(s: &str) -> Result<AnnotationAuthor> {
    match s {
        "ai" => Ok(AnnotationAuthor::Ai),
        "user" => Ok(AnnotationAuthor::User),
        other => Err(Error::Storage(format!("未知批注作者: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_tree_builds_sorted_nested_nodes() {
        let item = |id, parent_id| Item {
            id,
            workspace_id: 1,
            parent_id,
            kind: ItemKind::Note,
            name: format!("n{id}"),
            content: None,
            created_by: Creator::User,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        // 乱序输入：2（根，id 较大）、1（根）、3（挂在 1 下）。
        let items = vec![item(2, None), item(3, Some(1)), item(1, None)];
        let tree = assemble_tree(items);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].item.id, 1);
        assert_eq!(tree[1].item.id, 2);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].item.id, 3);
        assert!(tree[1].children.is_empty());
    }

    #[test]
    fn kind_creator_author_parse_known_and_reject_unknown() {
        assert_eq!(kind_from_str("dir").unwrap(), ItemKind::Dir);
        assert_eq!(kind_from_str("note").unwrap(), ItemKind::Note);
        assert!(kind_from_str("folder").is_err());
        assert_eq!(creator_from_str("agent").unwrap(), Creator::Agent);
        assert_eq!(creator_from_str("user").unwrap(), Creator::User);
        assert!(creator_from_str("bot").is_err());
        assert_eq!(author_from_str("ai").unwrap(), AnnotationAuthor::Ai);
        assert_eq!(author_from_str("user").unwrap(), AnnotationAuthor::User);
        assert!(author_from_str("admin").is_err());
    }
}
