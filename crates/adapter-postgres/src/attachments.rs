//! 附件输出端口实现：元数据走 PostgreSQL（attachments 表），二进制走宿主磁盘。
//!
//! 读路径在 SQL 层强制归属（items → workspaces join），作为隔离第二道防线；
//! 子树收集用 WITH RECURSIVE 自 anchor 行（join workspaces 限定 user）向下展开。
//! 磁盘布局 `{base}/{user_id}/{uuid}`：user_id 为数字、uuid 服务端生成且无扩展名，
//! 不存在路径注入面。删除幂等（文件缺失不算错，宿主手动清理的既定折衷）。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::error::{Error, Result};
use domain::ports::{AttachmentRepository, AttachmentStorage};
use domain::space::Attachment;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::map_sqlx_error;

pub struct PgAttachmentRepository {
    pool: PgPool,
}

impl PgAttachmentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AttachmentRepository for PgAttachmentRepository {
    async fn insert(&self, attachment: &Attachment) -> Result<i64> {
        let row = sqlx::query(
            "insert into attachments (item_id, filename, mime, size_bytes, uuid)
             values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(attachment.item_id)
        .bind(&attachment.filename)
        .bind(&attachment.mime)
        .bind(attachment.size_bytes)
        .bind(&attachment.uuid)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    // 归属守卫：items → workspaces join 限定 user（第二道防线）。
    async fn find_by_id(&self, id: i64, user_id: i64) -> Result<Option<Attachment>> {
        sqlx::query(
            "select a.id, a.item_id, a.filename, a.mime, a.size_bytes, a.uuid, a.created_at
             from attachments a
             join items i on i.id = a.item_id
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where a.id = $1",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| attachment_from_row(&row))
        .transpose()
    }

    async fn list_by_item(&self, item_id: i64, user_id: i64) -> Result<Vec<Attachment>> {
        let rows = sqlx::query(
            "select a.id, a.item_id, a.filename, a.mime, a.size_bytes, a.uuid, a.created_at
             from attachments a
             join items i on i.id = a.item_id
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where a.item_id = $1 order by a.id",
        )
        .bind(item_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(attachment_from_row).collect()
    }

    async fn list_by_workspace(&self, workspace_id: i64, user_id: i64) -> Result<Vec<Attachment>> {
        let rows = sqlx::query(
            "select a.id, a.item_id, a.filename, a.mime, a.size_bytes, a.uuid, a.created_at
             from attachments a
             join items i on i.id = a.item_id
             join workspaces w on w.id = i.workspace_id and w.user_id = $2
             where i.workspace_id = $1
             order by a.id",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(attachment_from_row).collect()
    }

    // 子树收集：自 anchor（join workspaces 限定归属）向下递归展开全部后代，
    // 再联 attachments——删除笔记前收集磁盘文件用。
    async fn list_by_item_tree(&self, item_id: i64, user_id: i64) -> Result<Vec<Attachment>> {
        let rows = sqlx::query(
            "with recursive subtree as (
               select i.id from items i
               join workspaces w on w.id = i.workspace_id and w.user_id = $2
               where i.id = $1
               union all
               select i.id from items i join subtree s on i.parent_id = s.id
             )
             select a.id, a.item_id, a.filename, a.mime, a.size_bytes, a.uuid, a.created_at
             from attachments a
             join subtree s on s.id = a.item_id
             order by a.id",
        )
        .bind(item_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(attachment_from_row).collect()
    }

    // 写路径守卫：join 限定归属（第二道防线）。
    async fn delete(&self, id: i64, user_id: i64) -> Result<bool> {
        let result = sqlx::query(
            "delete from attachments a using items i, workspaces w
             where a.id = $1 and i.id = a.item_id and w.id = i.workspace_id and w.user_id = $2",
        )
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    // 批量删行：不校验归属——调用方（服务端删笔记的级联清理）先经
    // list_by_item_tree 收集校验，这里只按 id 删。
    async fn delete_by_ids(&self, ids: &[i64]) -> Result<()> {
        sqlx::query("delete from attachments where id = any($1)")
            .bind(ids)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(())
    }
}

fn attachment_from_row(row: &PgRow) -> Result<Attachment> {
    Ok(Attachment {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        item_id: row.try_get::<i64, _>("item_id").map_err(map_sqlx_error)?,
        filename: row
            .try_get::<String, _>("filename")
            .map_err(map_sqlx_error)?,
        mime: row.try_get::<String, _>("mime").map_err(map_sqlx_error)?,
        size_bytes: row
            .try_get::<i64, _>("size_bytes")
            .map_err(map_sqlx_error)?,
        uuid: row.try_get::<String, _>("uuid").map_err(map_sqlx_error)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
    })
}

/// 附件二进制存储：`{base}/{user_id}/{uuid}`（uuid 无扩展名，防路径注入）。
pub struct FsAttachmentStorage {
    base: PathBuf,
}

impl FsAttachmentStorage {
    pub fn new(base: impl AsRef<Path>) -> Self {
        Self {
            base: base.as_ref().to_owned(),
        }
    }

    fn path(&self, user_id: i64, uuid: &str) -> PathBuf {
        self.base.join(user_id.to_string()).join(uuid)
    }
}

#[async_trait]
impl AttachmentStorage for FsAttachmentStorage {
    async fn store(&self, user_id: i64, uuid: &str, bytes: &[u8]) -> Result<()> {
        let dir = self.base.join(user_id.to_string());
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| Error::Storage(format!("创建附件目录失败: {e}")))?;
        tokio::fs::write(self.path(user_id, uuid), bytes)
            .await
            .map_err(|e| Error::Storage(format!("写入附件文件失败: {e}")))?;
        Ok(())
    }

    async fn load(&self, user_id: i64, uuid: &str) -> Result<Vec<u8>> {
        tokio::fs::read(self.path(user_id, uuid))
            .await
            .map_err(|e| Error::Storage(format!("读取附件文件失败: {e}")))
    }

    // 删除幂等：文件缺失不算错（宿主手动清理的既定折衷）。
    async fn delete(&self, user_id: i64, uuid: &str) -> Result<()> {
        match tokio::fs::remove_file(self.path(user_id, uuid)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Storage(format!("删除附件文件失败: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn storage_path_is_user_scoped_and_extensionless() {
        let storage = FsAttachmentStorage::new("/tmp/xueban-att-test");
        let uuid = Uuid::new_v4().to_string();
        let p = storage.path(7, &uuid);
        assert_eq!(p.parent().unwrap().file_name().unwrap(), "7");
        assert_eq!(p.file_name().unwrap(), uuid.as_str());
        assert_eq!(p.extension(), None);
    }
}
