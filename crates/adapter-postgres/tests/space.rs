//! 学习空间仓储集成测试（env-gated）：workspaces / items 树 / annotations 生命周期。

mod common;

use adapter_postgres::{
    PgAnnotationRepository, PgItemRepository, PgUserRepository, PgWorkspaceRepository,
};
use chrono::{NaiveDate, Utc};
use common::{insert_user, pool, setup, stamp};
use domain::ports::{AnnotationRepository, ItemRepository, WorkspaceRepository};
use domain::space::{Annotation, AnnotationAuthor, Creator, Item, ItemKind, Workspace};

fn new_workspace(user_id: i64) -> Workspace {
    Workspace {
        id: 0,
        user_id,
        name: format!("空间_{}", stamp()),
        exam_goal: "通过考试".into(),
        exam_date: None,
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn workspace_insert_find_and_list_by_user() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "ws").await;
    let other = insert_user(&user_repo, "ws2").await;
    let repo = PgWorkspaceRepository::new(pool);

    let first = repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let second = repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    repo.insert(&new_workspace(other))
        .await
        .expect("插入空间失败");

    let found = repo
        .find_by_id_and_user(first, user_id)
        .await
        .expect("查询失败")
        .expect("应按 id 找到");
    assert_eq!(found.id, first);
    assert_eq!(found.user_id, user_id);
    // 跨用户访问被 SQL 归属条件挡掉。
    assert!(
        repo.find_by_id_and_user(first, other)
            .await
            .expect("查询失败")
            .is_none()
    );
    assert!(
        repo.find_by_id_and_user(999_999, user_id)
            .await
            .expect("查询失败")
            .is_none()
    );

    let mine = repo.list_by_user(user_id).await.expect("列表失败");
    assert_eq!(mine.len(), 2);
    assert!(mine.iter().any(|w| w.id == first));
    assert!(mine.iter().any(|w| w.id == second));
    assert!(mine.iter().all(|w| w.user_id == user_id));
}

#[tokio::test]
async fn workspace_update_goal_and_date() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "ws").await;
    let repo = PgWorkspaceRepository::new(pool);

    let id = repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let mut ws = repo
        .find_by_id_and_user(id, user_id)
        .await
        .expect("查询失败")
        .expect("应找到");
    ws.set_goal(
        "考研".into(),
        "总分 400".into(),
        Some(NaiveDate::from_ymd_opt(2026, 12, 20).unwrap()),
    );
    repo.update(&ws).await.expect("更新失败");

    let after = repo
        .find_by_id_and_user(id, user_id)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_eq!(after.name, "考研");
    assert_eq!(after.exam_goal, "总分 400");
    assert_eq!(
        after.exam_date,
        Some(NaiveDate::from_ymd_opt(2026, 12, 20).unwrap())
    );
}

#[tokio::test]
async fn item_tree_insert_list_and_ownership() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "tree").await;
    let other = insert_user(&user_repo, "tree2").await;
    let ws_repo = PgWorkspaceRepository::new(pool.clone());
    let ws_id = ws_repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let other_ws = ws_repo
        .insert(&new_workspace(other))
        .await
        .expect("插入空间失败");
    let repo = PgItemRepository::new(pool);

    let item = |parent_id, kind, name: &str| Item {
        id: 0,
        workspace_id: ws_id,
        parent_id,
        kind,
        name: name.into(),
        content: if kind == ItemKind::Note {
            Some("# 正文".into())
        } else {
            None
        },
        created_by: Creator::User,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let root = repo
        .insert(&item(None, ItemKind::Dir, "根"))
        .await
        .expect("插入失败");
    let child = repo
        .insert(&item(Some(root), ItemKind::Note, "笔记A"))
        .await
        .expect("插入失败");
    let grand = repo
        .insert(&item(Some(child), ItemKind::Note, "笔记B"))
        .await
        .expect("插入失败");
    repo.insert(&item(None, ItemKind::Dir, "第二根"))
        .await
        .expect("插入失败");

    // 子节点查询：根目录的子节点只有 child，空父查询两者皆有。
    let children = repo
        .list_children(ws_id, Some(root))
        .await
        .expect("查询失败");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, child);
    let roots = repo.list_children(ws_id, None).await.expect("查询失败");
    assert_eq!(roots.len(), 2);

    // 完整树：深度组装 + id 升序。
    let tree = repo.list_tree(ws_id, user_id).await.expect("树查询失败");
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].item.name, "根");
    assert_eq!(tree[0].children.len(), 1);
    assert_eq!(tree[0].children[0].item.name, "笔记A");
    assert_eq!(tree[0].children[0].children[0].item.id, grand);

    // 归属：他人空间查不到树；他人查不到本空间节点。
    let foreign_tree = repo.list_tree(ws_id, other).await.expect("查询失败");
    assert!(foreign_tree.is_empty());
    assert!(
        repo.list_tree(other_ws, user_id)
            .await
            .expect("查询失败")
            .is_empty()
    );
    assert!(
        repo.find_by_id(child, other)
            .await
            .expect("查询失败")
            .is_none()
    );
    let owned = repo.find_by_id(child, user_id).await.expect("查询失败");
    assert_eq!(owned.expect("应按归属找到").id, child);
}

#[tokio::test]
async fn item_update_move_and_ancestors_chain() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "anc").await;
    let ws_repo = PgWorkspaceRepository::new(pool.clone());
    let ws_id = ws_repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let repo = PgItemRepository::new(pool);

    let make = |parent_id, name: &str| Item {
        id: 0,
        workspace_id: ws_id,
        parent_id,
        kind: ItemKind::Note,
        name: name.into(),
        content: Some("正文".into()),
        created_by: Creator::User,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let a = repo.insert(&make(None, "A")).await.expect("插入失败");
    let b = repo.insert(&make(Some(a), "B")).await.expect("插入失败");
    let c = repo.insert(&make(Some(b), "C")).await.expect("插入失败");

    // 根→自身 链：[A, B, C]。
    let chain = repo.ancestors(c).await.expect("祖先链失败");
    assert_eq!(chain, vec![a, b, c]);
    let chain_root = repo.ancestors(a).await.expect("祖先链失败");
    assert_eq!(chain_root, vec![a]);

    // 移动：C 挂到 A 下。
    let mut moved = repo
        .find_by_id(c, user_id)
        .await
        .expect("查询失败")
        .expect("应找到");
    moved.parent_id = Some(a);
    repo.update(&moved).await.expect("更新失败");
    let chain_after = repo.ancestors(c).await.expect("祖先链失败");
    assert_eq!(chain_after, vec![a, c]);
}

#[tokio::test]
async fn annotation_lifecycle_and_ownership_delete() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "ann").await;
    let other = insert_user(&user_repo, "ann2").await;
    let ws_repo = PgWorkspaceRepository::new(pool.clone());
    let ws_id = ws_repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let item_repo = PgItemRepository::new(pool.clone());
    let note = item_repo
        .insert(&Item {
            id: 0,
            workspace_id: ws_id,
            parent_id: None,
            kind: ItemKind::Note,
            name: "笔记".into(),
            content: Some("# 标题".into()),
            created_by: Creator::User,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .await
        .expect("插入笔记失败");
    let repo = PgAnnotationRepository::new(pool);

    let ann = |author| Annotation {
        id: 0,
        item_id: note,
        user_id,
        author,
        anchor: "标题".into(),
        text: "批注内容".into(),
        created_at: Utc::now(),
    };
    let id1 = repo
        .insert(&ann(AnnotationAuthor::User))
        .await
        .expect("插入失败");
    let id2 = repo
        .insert(&ann(AnnotationAuthor::Ai))
        .await
        .expect("插入失败");

    let list = repo.list_by_item(note).await.expect("查询失败");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, id1);
    assert_eq!(list[1].id, id2);
    assert_eq!(list[1].author, AnnotationAuthor::Ai);

    // 他人不能删（user_id 归属校验）。
    assert!(!repo.delete(id1, other).await.expect("删除失败"));
    // 本人可删；重复删返回 false。
    assert!(repo.delete(id1, user_id).await.expect("删除失败"));
    assert!(!repo.delete(id1, user_id).await.expect("重复删除"));
    assert_eq!(repo.list_by_item(note).await.expect("查询失败").len(), 1);
}
