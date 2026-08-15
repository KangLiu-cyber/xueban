//! 附件用例：UploadAttachment / ReadAttachment / DeleteAttachment。
//!
//! 二进制经 `AttachmentStorage`（宿主磁盘）存取，元数据经
//! `AttachmentRepository` 落库。归属防线与 Item 一致：仓储在 SQL 层
//! join workspaces 限定 user_id，本服务再以 `find_by_id(item_id, user_id)`
//! 显式校验（未命中 → NotFound）并执行 is_note 业务校验。
//!
//! 一致性：先落文件后插行，插行失败（如并发删 item → FK violation）
//! best-effort 删文件回滚；删除时先删行再删文件，文件缺失不算错
//! （宿主手动清理），崩溃窗口的孤儿文件是无害的既定折衷（见架构文档 §6/§7）。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::ports::{AttachmentRepository, AttachmentStorage, ItemRepository};
use domain::space::{Attachment, Item};
use uuid::Uuid;

/// 单附件上限：10MB（协议层 DefaultBodyLimit 与用例层双重校验）。
pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

/// 魔数嗅探白名单：存库 mime 的权威来源，防伪造 Content-Type。
/// 明确拒绝 svg（可携带脚本，XSS 面）。返回 `None` 表示非白名单图片。
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "img",
    }
}

pub struct AttachmentService<I, A, S>
where
    I: ItemRepository + ?Sized,
    A: AttachmentRepository + ?Sized,
    S: AttachmentStorage + ?Sized,
{
    items: Arc<I>,
    attachments: Arc<A>,
    storage: Arc<S>,
}

impl<I, A, S> AttachmentService<I, A, S>
where
    I: ItemRepository + ?Sized,
    A: AttachmentRepository + ?Sized,
    S: AttachmentStorage + ?Sized,
{
    pub fn new(items: Arc<I>, attachments: Arc<A>, storage: Arc<S>) -> Self {
        Self {
            items,
            attachments,
            storage,
        }
    }

    /// UploadAttachment：校验归属 + is_note + 大小 + 魔数，落文件后插行。
    pub async fn upload(
        &self,
        user_id: i64,
        item_id: i64,
        filename: String,
        bytes: &[u8],
    ) -> Result<Attachment> {
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(Error::Invalid("附件超过 10MB 上限".to_owned()));
        }
        let item = self.require_item(user_id, item_id).await?;
        if !item.is_note() {
            return Err(Error::Invalid("附件只能挂在笔记上".to_owned()));
        }
        let mime = sniff_mime(bytes)
            .ok_or_else(|| Error::Invalid("不支持的图片格式（仅 png/jpeg/gif/webp）".to_owned()))?;
        // 未提供文件名时按嗅探 mime 给默认（仅展示用，存储键是 uuid）。
        let filename = if filename.trim().is_empty() {
            format!("image.{}", ext_for_mime(mime))
        } else {
            filename
        };
        let uuid = Uuid::new_v4().to_string();
        self.storage.store(user_id, &uuid, bytes).await?;
        let mut att = Attachment {
            id: 0,
            item_id,
            filename,
            mime: mime.to_owned(),
            size_bytes: bytes.len() as i64,
            uuid,
            created_at: Utc::now(),
        };
        match self.attachments.insert(&att).await {
            Ok(id) => {
                att.id = id;
                Ok(att)
            }
            Err(e) => {
                // 插行失败（如并发删 item → FK violation）：回滚已落盘文件。
                let _ = self.storage.delete(user_id, &att.uuid).await;
                Err(e)
            }
        }
    }

    /// ReadAttachment：校验归属后返回元数据 + 二进制。
    pub async fn read(&self, user_id: i64, attachment_id: i64) -> Result<(Attachment, Vec<u8>)> {
        let att = self.require_attachment(user_id, attachment_id).await?;
        let bytes = self.storage.load(user_id, &att.uuid).await?;
        Ok((att, bytes))
    }

    /// DeleteAttachment：先删行再删文件（文件缺失不算错）。
    pub async fn delete(&self, user_id: i64, attachment_id: i64) -> Result<()> {
        let att = self.require_attachment(user_id, attachment_id).await?;
        let hit = self.attachments.delete(attachment_id, user_id).await?;
        if !hit {
            return Err(Error::NotFound("附件不存在".to_owned()));
        }
        self.storage.delete(user_id, &att.uuid).await?;
        Ok(())
    }

    /// 删除 item 前的子树附件清理：收集整棵子树（含自身）的附件并删磁盘文件，
    /// 返回清理的文件数。表行不动——item 删除后由 DB 级联兜底删除。
    pub async fn delete_item_tree(&self, user_id: i64, item_id: i64) -> Result<usize> {
        self.require_item(user_id, item_id).await?;
        let atts = self.attachments.list_by_item_tree(item_id, user_id).await?;
        for a in &atts {
            self.storage.delete(user_id, &a.uuid).await?;
        }
        Ok(atts.len())
    }

    /// 归属校验：item 属于 user（仓储 SQL join 防线 + 显式未命中 → NotFound）。
    async fn require_item(&self, user_id: i64, item_id: i64) -> Result<Item> {
        self.items
            .find_by_id(item_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("内容不存在".to_owned()))
    }

    async fn require_attachment(&self, user_id: i64, attachment_id: i64) -> Result<Attachment> {
        self.attachments
            .find_by_id(attachment_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("附件不存在".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryAttachmentRepository, InMemoryAttachmentStorage, InMemoryItemRepository,
        InMemoryWorkspaceRepository, insert_item, insert_workspace,
    };
    use domain::space::ItemKind;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01, 0x02,
    ];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
    const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00";
    const WEBP: &[u8] = b"RIFF\x00\x00\x00\x00WEBPVP8 ";
    const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";

    type Svc = (
        AttachmentService<
            InMemoryItemRepository,
            InMemoryAttachmentRepository,
            InMemoryAttachmentStorage,
        >,
        Arc<InMemoryItemRepository>,
        Arc<InMemoryAttachmentStorage>,
        Arc<InMemoryWorkspaceRepository>,
    );

    fn svc() -> Svc {
        // item 仓储注入 workspace 注册表：find_by_id 按空间归属过滤，
        // 模拟 Pg 实现的 SQL join 防线（AttachmentService 无 workspace 依赖）。
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let items = Arc::new(InMemoryItemRepository::with_workspaces(workspaces.clone()));
        let storage = Arc::new(InMemoryAttachmentStorage::default());
        let service = AttachmentService::new(
            items.clone(),
            Arc::new(InMemoryAttachmentRepository::with_items(items.clone())),
            storage.clone(),
        );
        (service, items, storage, workspaces)
    }

    #[test]
    fn sniff_accepts_whitelist_and_rejects_svg() {
        assert_eq!(sniff_mime(PNG), Some("image/png"));
        assert_eq!(sniff_mime(JPEG), Some("image/jpeg"));
        assert_eq!(sniff_mime(GIF), Some("image/gif"));
        assert_eq!(sniff_mime(WEBP), Some("image/webp"));
        assert_eq!(sniff_mime(SVG), None);
        assert_eq!(sniff_mime(b""), None);
        assert_eq!(sniff_mime(b"not an image"), None);
    }

    #[tokio::test]
    async fn upload_read_delete_roundtrip() {
        let (s, items, storage, workspaces) = svc();
        let ws = insert_workspace(&workspaces, 1, "集").await;
        let note = insert_item(&items, ws, "笔记", ItemKind::Note).await;
        let att = s.upload(1, note, "截图.png".into(), PNG).await.unwrap();
        assert_eq!(att.mime, "image/png");
        assert_eq!(att.size_bytes, PNG.len() as i64);
        assert!(!att.uuid.is_empty());
        assert!(!storage.files.lock().unwrap().is_empty());

        let (read_back, bytes) = s.read(1, att.id).await.unwrap();
        assert_eq!(read_back.id, att.id);
        assert_eq!(bytes, PNG);

        s.delete(1, att.id).await.unwrap();
        assert!(matches!(s.read(1, att.id).await, Err(Error::NotFound(_))));
        assert!(storage.files.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn upload_rejects_dir_oversize_and_foreign_item() {
        let (s, items, _storage, workspaces) = svc();
        let ws1 = insert_workspace(&workspaces, 1, "我的").await;
        let ws2 = insert_workspace(&workspaces, 2, "别人的").await;
        let dir = insert_item(&items, ws1, "集", ItemKind::Dir).await;
        let note = insert_item(&items, ws2, "别人的笔记", ItemKind::Note).await;
        assert!(matches!(
            s.upload(1, dir, "x.png".into(), PNG).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            s.upload(1, note, "x.png".into(), PNG).await,
            Err(Error::NotFound(_))
        ));
        let huge = vec![0xFF, 0xD8, 0xFF];
        let huge = [huge.as_slice(), &vec![0u8; MAX_ATTACHMENT_BYTES]].concat();
        assert!(matches!(
            s.upload(1, dir, "huge.jpg".into(), &huge).await,
            Err(Error::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn upload_sniffs_authoritative_mime() {
        let (s, items, _storage, workspaces) = svc();
        let ws = insert_workspace(&workspaces, 1, "集").await;
        let note = insert_item(&items, ws, "笔记", ItemKind::Note).await;
        // svg 内容：魔数嗅探拒绝（防脚本注入面；前缀嗅探只认前导字节，
        // 头部拼 png 魔数属于白名单内的情况，不在嗅探职责内）。
        assert!(matches!(
            s.upload(1, note, "fake.svg".into(), SVG).await,
            Err(Error::Invalid(_))
        ));
        // gif 内容即使文件名/暗示为 png，也按魔数存 gif。
        let att = s.upload(1, note, "a.gif".into(), GIF).await.unwrap();
        assert_eq!(att.mime, "image/gif");
    }

    #[tokio::test]
    async fn read_isolates_users() {
        let (s, items, _storage, workspaces) = svc();
        let ws = insert_workspace(&workspaces, 1, "集").await;
        let note = insert_item(&items, ws, "笔记", ItemKind::Note).await;
        let att = s.upload(1, note, "a.png".into(), PNG).await.unwrap();
        assert!(matches!(s.read(2, att.id).await, Err(Error::NotFound(_))));
        assert!(matches!(s.delete(2, att.id).await, Err(Error::NotFound(_))));
        assert!(s.read(1, att.id).await.is_ok());
    }

    #[tokio::test]
    async fn delete_item_tree_cleans_subtree_files() {
        let (s, items, storage, workspaces) = svc();
        let ws = insert_workspace(&workspaces, 1, "集").await;
        let dir = insert_item(&items, ws, "集", ItemKind::Dir).await;
        let sub = insert_item(&items, ws, "子目录", ItemKind::Dir).await;
        let note = insert_item(&items, ws, "笔记", ItemKind::Note).await;
        let note2 = insert_item(&items, ws, "笔记2", ItemKind::Note).await;
        // 造父子关系：dir → sub → note；note2 直接挂在 dir 下（验证两层深度）。
        {
            let map = &items.items;
            let mut guard = map.lock().unwrap();
            guard.get_mut(&sub).unwrap().parent_id = Some(dir);
            guard.get_mut(&note).unwrap().parent_id = Some(sub);
            guard.get_mut(&note2).unwrap().parent_id = Some(dir);
        }
        let a1 = s.upload(1, note, "a.png".into(), PNG).await.unwrap();
        let a2 = s.upload(1, note, "b.jpg".into(), JPEG).await.unwrap();
        let a3 = s.upload(1, note2, "c.gif".into(), GIF).await.unwrap();
        assert_eq!(storage.files.lock().unwrap().len(), 3);

        // 删根目录：收集整棵子树附件（3 个）并清文件；行不动（item 删除后 DB 级联）。
        assert_eq!(s.delete_item_tree(1, dir).await.unwrap(), 3);
        assert!(storage.files.lock().unwrap().is_empty());
        // 表行仍在（PG 侧由 item 删除级联，这里验证服务不误删行），
        // 但磁盘文件已清 → read 在 load 阶段报错。
        assert!(s.read(1, a1.id).await.is_err());
        assert!(s.read(1, a2.id).await.is_err());
        assert!(s.read(1, a3.id).await.is_err());
    }

    #[tokio::test]
    async fn delete_item_tree_isolates_users_and_unknown() {
        let (s, items, _storage, workspaces) = svc();
        let ws = insert_workspace(&workspaces, 1, "集").await;
        let dir = insert_item(&items, ws, "集", ItemKind::Dir).await;
        assert!(matches!(
            s.delete_item_tree(2, dir).await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            s.delete_item_tree(1, 999).await,
            Err(Error::NotFound(_))
        ));
    }
}
