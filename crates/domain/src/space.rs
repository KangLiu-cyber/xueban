//! 学习空间上下文：Workspace 聚合、Item 内容树、Annotation 批注。

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    /// 手写考试目标（自由文本，不做枚举）。
    pub exam_goal: String,
    pub exam_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
}

impl Workspace {
    /// ManageExamGoal：更新空间信息（名称、目标、日期），日期可为空。
    pub fn set_goal(&mut self, name: String, exam_goal: String, exam_date: Option<NaiveDate>) {
        self.name = name;
        self.exam_goal = exam_goal;
        self.exam_date = exam_date;
    }
}

/// 内容节点类型：目录或笔记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Dir,
    Note,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Dir => "dir",
            ItemKind::Note => "note",
        }
    }
}

/// 内容创建者：Agent 生成的内容打 ai 来源标记，供客户端展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Creator {
    Agent,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub workspace_id: i64,
    /// 空为根节点，parent_id 任意嵌套成树。
    pub parent_id: Option<i64>,
    pub kind: ItemKind,
    pub name: String,
    /// note 的 Markdown 正文（dir 无正文）。
    pub content: Option<String>,
    pub created_by: Creator,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Item {
    pub fn is_dir(&self) -> bool {
        self.kind == ItemKind::Dir
    }

    pub fn is_note(&self) -> bool {
        self.kind == ItemKind::Note
    }
}

/// 树形输出：仓储按递归查询组装，应用层/适配器直接返回。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemNode {
    pub item: Item,
    pub children: Vec<ItemNode>,
}

/// 防环不变式（领域服务）：把 `item_id` 移动到 `new_parent` 时，
/// `ancestors` 是从根到新父节点的祖先 id 链（含新父自身）。
/// 若链中出现 `item_id` 本身，则新父是自己的后代，拒绝移动。
///
/// 创建路径复用同一守卫：新建节点 id 尚未分配，以占位 0 传入——0 永不与
/// 真实节点 id 冲突（仓储 id 从 1 起），健康树上必然通过，从而把不变式
/// 挂在所有写入口；未来补移动用例时该守卫直接生效。
pub fn assert_no_cycle(new_parent: Option<i64>, item_id: i64, ancestors: &[i64]) -> Result<()> {
    let Some(parent) = new_parent else {
        return Ok(());
    };
    if parent == item_id || ancestors.contains(&item_id) {
        return Err(Error::Invalid(
            "目录不能移动到自身的子节点下（防环）".to_owned(),
        ));
    }
    Ok(())
}

/// 批注作者：AI 批注不可被用户修改只能删除；用户批注可编辑删除。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationAuthor {
    Ai,
    User,
}

impl AnnotationAuthor {
    pub fn as_str(self) -> &'static str {
        match self {
            AnnotationAuthor::Ai => "ai",
            AnnotationAuthor::User => "user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: i64,
    pub item_id: i64,
    pub user_id: i64,
    pub author: AnnotationAuthor,
    /// 正文引用片段（定位锚点）。
    pub anchor: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

/// 笔记图片附件（只挂 note）。二进制存宿主磁盘 `{user_id}/{uuid}`，
/// 本实体只携带元数据；归属经 item → workspace join 校验（同 Item，
/// 不直接存 user_id）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: i64,
    pub item_id: i64,
    /// 原始文件名（仅展示用）。
    pub filename: String,
    /// 以魔数嗅探结果为准（白名单 png/jpeg/gif/webp）。
    pub mime: String,
    pub size_bytes: i64,
    /// 磁盘文件名（服务端生成，无扩展名，防路径注入）。
    pub uuid: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(user_id: i64) -> Workspace {
        Workspace {
            id: 1,
            user_id,
            name: "我的备考".into(),
            exam_goal: "通过考试".into(),
            exam_date: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn set_goal_updates_fields() {
        let mut ws = workspace(1);
        ws.set_goal(
            "考研".into(),
            "总分 400".into(),
            Some(NaiveDate::from_ymd_opt(2026, 12, 20).unwrap()),
        );
        assert_eq!(ws.name, "考研");
        assert_eq!(ws.exam_goal, "总分 400");
        assert_eq!(
            ws.exam_date,
            Some(NaiveDate::from_ymd_opt(2026, 12, 20).unwrap())
        );
    }

    #[test]
    fn set_goal_clears_date() {
        let mut ws = workspace(1);
        ws.exam_date = Some(NaiveDate::from_ymd_opt(2026, 12, 20).unwrap());
        ws.set_goal("备考".into(), "目标".into(), None);
        assert_eq!(ws.exam_date, None);
    }

    #[test]
    fn no_cycle_allows_root_move() {
        // 移到根：无祖先，不涉及环。
        assert!(assert_no_cycle(None, 3, &[]).is_ok());
    }

    #[test]
    fn no_cycle_rejects_self_parent() {
        assert!(assert_no_cycle(Some(3), 3, &[]).is_err());
    }

    #[test]
    fn no_cycle_rejects_move_into_own_descendant() {
        // 树：1 → 2 → 3；把 2 挂到 3 下（新父 3 的祖先链 [1,2,3] 含 2）→ 非法。
        assert!(assert_no_cycle(Some(3), 2, &[1, 2, 3]).is_err());
        // 把 3 挂到 1 下（新父 1 的祖先链 [1] 不含 3，只是祖父）→ 合法。
        assert!(assert_no_cycle(Some(1), 3, &[1]).is_ok());
    }

    #[test]
    fn no_cycle_allows_keeping_current_parent() {
        // 树：1 → 2 → 3；把 3 挂回原父 2（新父 2 的祖先链 [1,2] 不含 3）→ 合法。
        assert!(assert_no_cycle(Some(2), 3, &[1, 2]).is_ok());
    }

    #[test]
    fn item_kind_discriminates() {
        let note = Item {
            id: 1,
            workspace_id: 1,
            parent_id: None,
            kind: ItemKind::Note,
            name: "笔记".into(),
            content: Some("# 标题".into()),
            created_by: Creator::Agent,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(note.is_note());
        assert!(!note.is_dir());
    }
}
