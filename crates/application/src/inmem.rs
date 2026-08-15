//! 内存仓储替身：应用层单元测试用（单测编译进 crate，不随发布构建）。
//!
//! 每个仓储独立持有 `Mutex<HashMap>`，id 用 `AtomicI64` 自增；
//! 与 SQLx 适配器语义对齐：insert 返回新 id、查询按归属过滤、删除返回是否命中。

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Utc};
use domain::event::Event;
use domain::identity::{Token, TokenPurpose, User};
use domain::ports::{
    AnnotationRepository, CredentialIssuer, EventStore, ItemRepository, PaperRepository,
    PasswordHasher, QuestionRepository, QuizRecordRepository, TokenRepository, UserRepository,
    WorkspaceRepository, WrongItemRepository,
};
use domain::practice::{Paper, Question, QuestionType, QuizRecord, WrongItem, WrongStats};
use domain::space::{Annotation, Item, ItemKind, ItemNode, Workspace};

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn take_id(next: &AtomicI64) -> i64 {
    next.fetch_add(1, Ordering::SeqCst)
}

#[derive(Default)]
pub struct InMemoryUserRepository {
    users: Mutex<HashMap<i64, User>>,
    next_id: AtomicI64,
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn insert(&self, user: &User) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut u = user.clone();
        u.id = id;
        self.users.lock().unwrap().insert(id, u);
        Ok(id)
    }

    async fn find_by_account(&self, account: &str) -> domain::Result<Option<User>> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.account == account)
            .cloned())
    }

    async fn find_by_id(&self, id: i64) -> domain::Result<Option<User>> {
        Ok(self.users.lock().unwrap().get(&id).cloned())
    }
}

#[derive(Default)]
pub struct InMemoryTokenRepository {
    tokens: Mutex<HashMap<i64, Token>>,
    next_id: AtomicI64,
}

#[async_trait]
impl TokenRepository for InMemoryTokenRepository {
    async fn insert(&self, token: &Token) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut t = token.clone();
        t.id = id;
        self.tokens.lock().unwrap().insert(id, t);
        Ok(id)
    }

    async fn find_by_token(&self, token: &str) -> domain::Result<Option<Token>> {
        Ok(self
            .tokens
            .lock()
            .unwrap()
            .values()
            .find(|t| t.token == token)
            .cloned())
    }

    async fn revoke(&self, token: &str, revoked_at: DateTime<Utc>) -> domain::Result<()> {
        if let Some(t) = self
            .tokens
            .lock()
            .unwrap()
            .values_mut()
            .find(|t| t.token == token)
        {
            t.revoked_at = Some(revoked_at);
        }
        Ok(())
    }

    async fn revoke_by_user_purpose(
        &self,
        user_id: i64,
        purpose: TokenPurpose,
        revoked_at: DateTime<Utc>,
    ) -> domain::Result<()> {
        for t in self
            .tokens
            .lock()
            .unwrap()
            .values_mut()
            .filter(|t| t.user_id == user_id && t.purpose == purpose)
        {
            t.revoked_at = Some(revoked_at);
        }
        Ok(())
    }

    async fn find_active_by_user_purpose(
        &self,
        user_id: i64,
        purpose: TokenPurpose,
    ) -> domain::Result<Option<Token>> {
        Ok(self
            .tokens
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.user_id == user_id && t.purpose == purpose && t.revoked_at.is_none())
            .max_by_key(|t| t.id)
            .cloned())
    }
}

#[derive(Default)]
pub struct InMemoryWorkspaceRepository {
    workspaces: Mutex<HashMap<i64, Workspace>>,
    next_id: AtomicI64,
}

#[async_trait]
impl WorkspaceRepository for InMemoryWorkspaceRepository {
    async fn insert(&self, ws: &Workspace) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut w = ws.clone();
        w.id = id;
        self.workspaces.lock().unwrap().insert(id, w);
        Ok(id)
    }

    async fn find_by_id_and_user(
        &self,
        id: i64,
        user_id: i64,
    ) -> domain::Result<Option<Workspace>> {
        Ok(self
            .workspaces
            .lock()
            .unwrap()
            .get(&id)
            .filter(|w| w.user_id == user_id)
            .cloned())
    }

    async fn list_by_user(&self, user_id: i64) -> domain::Result<Vec<Workspace>> {
        let mut list: Vec<Workspace> = self
            .workspaces
            .lock()
            .unwrap()
            .values()
            .filter(|w| w.user_id == user_id)
            .cloned()
            .collect();
        list.sort_by_key(|w| w.id);
        Ok(list)
    }

    async fn update(&self, ws: &Workspace) -> domain::Result<()> {
        self.workspaces.lock().unwrap().insert(ws.id, ws.clone());
        Ok(())
    }
}

pub struct InMemoryItemRepository {
    items: Mutex<HashMap<i64, Item>>,
    next_id: AtomicI64,
}

impl Default for InMemoryItemRepository {
    fn default() -> Self {
        Self {
            items: Mutex::new(HashMap::new()),
            // id 从 1 开始，与 PG bigserial 语义一致；0 保留为未插入占位
            //（create 防环检查以占位 id 0 调用 assert_no_cycle，不能与真实节点冲突）。
            next_id: AtomicI64::new(1),
        }
    }
}

impl InMemoryItemRepository {
    fn tree(&self, items: &[Item]) -> Vec<ItemNode> {
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
        build(None, items)
    }
}

#[async_trait]
impl ItemRepository for InMemoryItemRepository {
    async fn insert(&self, item: &Item) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut i = item.clone();
        i.id = id;
        self.items.lock().unwrap().insert(id, i);
        Ok(id)
    }

    // item 不存 user_id，归属校验由应用层 read_item 完成（与 Pg 实现 SQL join 的防线等价）。
    async fn update(&self, item: &Item, _user_id: i64) -> domain::Result<()> {
        self.items.lock().unwrap().insert(item.id, item.clone());
        Ok(())
    }

    // item 不存 user_id，归属校验由应用层查 Workspace 完成。
    async fn find_by_id(&self, id: i64, _user_id: i64) -> domain::Result<Option<Item>> {
        Ok(self.items.lock().unwrap().get(&id).cloned())
    }

    // item 不存 user_id，归属校验由应用层查 Workspace 完成（与 Pg 实现 SQL join 的防线等价）。
    async fn list_children(
        &self,
        workspace_id: i64,
        _user_id: i64,
        parent_id: Option<i64>,
    ) -> domain::Result<Vec<Item>> {
        let mut list: Vec<Item> = self
            .items
            .lock()
            .unwrap()
            .values()
            .filter(|i| i.workspace_id == workspace_id && i.parent_id == parent_id)
            .cloned()
            .collect();
        list.sort_by_key(|i| i.id);
        Ok(list)
    }

    async fn list_tree(&self, workspace_id: i64, _user_id: i64) -> domain::Result<Vec<ItemNode>> {
        let items: Vec<Item> = self
            .items
            .lock()
            .unwrap()
            .values()
            .filter(|i| i.workspace_id == workspace_id)
            .cloned()
            .collect();
        Ok(self.tree(&items))
    }

    // item 不存 user_id，归属校验由应用层查 Workspace 完成（与 Pg 实现 SQL join 的防线等价）。
    async fn ancestors(&self, item_id: i64, _user_id: i64) -> domain::Result<Vec<i64>> {
        let items = self.items.lock().unwrap();
        let mut chain = vec![item_id];
        let mut cur = items.get(&item_id).and_then(|i| i.parent_id);
        while let Some(parent) = cur {
            if chain.contains(&parent) {
                break; // 防自环：损坏树终止，避免无限循环
            }
            chain.push(parent);
            cur = items.get(&parent).and_then(|i| i.parent_id);
        }
        chain.reverse();
        Ok(chain)
    }

    // 模拟 SQL ON DELETE CASCADE 的子树级联：BFS 收集全部后代一并删除。
    // 归属校验由应用层 read_item 完成（与 Pg 实现 SQL join 的防线等价）。
    async fn delete(&self, id: i64, _user_id: i64) -> domain::Result<bool> {
        let mut map = self.items.lock().unwrap();
        if !map.contains_key(&id) {
            return Ok(false);
        }
        let mut doomed = vec![id];
        let mut i = 0;
        while i < doomed.len() {
            let parent = doomed[i];
            doomed.extend(
                map.values()
                    .filter(|item| item.parent_id == Some(parent))
                    .map(|item| item.id),
            );
            i += 1;
        }
        for item_id in doomed {
            map.remove(&item_id);
        }
        Ok(true)
    }
}

#[derive(Default)]
pub struct InMemoryAnnotationRepository {
    annotations: Mutex<HashMap<i64, Annotation>>,
    next_id: AtomicI64,
}

#[async_trait]
impl AnnotationRepository for InMemoryAnnotationRepository {
    async fn insert(&self, ann: &Annotation) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut a = ann.clone();
        a.id = id;
        self.annotations.lock().unwrap().insert(id, a);
        Ok(id)
    }

    async fn list_by_item(&self, item_id: i64, _user_id: i64) -> domain::Result<Vec<Annotation>> {
        // 归属校验由应用层 read_item 完成（与 Pg 实现 SQL join 的防线等价）。
        let mut list: Vec<Annotation> = self
            .annotations
            .lock()
            .unwrap()
            .values()
            .filter(|a| a.item_id == item_id)
            .cloned()
            .collect();
        list.sort_by_key(|a| a.id);
        Ok(list)
    }

    async fn delete(&self, id: i64, user_id: i64) -> domain::Result<bool> {
        let mut map = self.annotations.lock().unwrap();
        let hit = map.get(&id).is_some_and(|a| a.user_id == user_id);
        if hit {
            map.remove(&id);
        }
        Ok(hit)
    }
}

#[derive(Default)]
pub struct InMemoryQuestionRepository {
    questions: Mutex<HashMap<i64, Question>>,
    next_id: AtomicI64,
}

#[async_trait]
impl QuestionRepository for InMemoryQuestionRepository {
    async fn insert_many(&self, questions: &[Question]) -> domain::Result<Vec<i64>> {
        let mut map = self.questions.lock().unwrap();
        let mut ids = Vec::with_capacity(questions.len());
        for q in questions {
            let id = take_id(&self.next_id);
            let mut copy = q.clone();
            copy.id = id;
            map.insert(id, copy);
            ids.push(id);
        }
        Ok(ids)
    }

    async fn find_by_ids(&self, ids: &[i64], user_id: i64) -> domain::Result<Vec<Question>> {
        let _ = user_id; // 归属校验在应用层
        let map = self.questions.lock().unwrap();
        let mut out: Vec<Question> = ids.iter().filter_map(|id| map.get(id).cloned()).collect();
        out.sort_by_key(|q| q.id);
        Ok(out)
    }

    async fn draw(
        &self,
        workspace_id: i64,
        _user_id: i64,
        source_item_ids: &[i64],
        qtypes: &[QuestionType],
        count: u32,
    ) -> domain::Result<Vec<Question>> {
        // 归属校验由应用层查 Workspace 完成（与 Pg 实现 SQL join 的防线等价）。
        let map = self.questions.lock().unwrap();
        let mut out: Vec<Question> = map
            .values()
            .filter(|q| {
                q.workspace_id == workspace_id
                    && (source_item_ids.is_empty() || source_item_ids.contains(&q.source_item_id))
                    && (qtypes.is_empty() || qtypes.contains(&q.qtype))
            })
            .cloned()
            .collect();
        out.sort_by_key(|q| q.id);
        out.truncate(count as usize);
        Ok(out)
    }
}

#[derive(Default)]
pub struct InMemoryQuizRecordRepository {
    records: Mutex<Vec<QuizRecord>>,
    next_id: AtomicI64,
}

#[async_trait]
impl QuizRecordRepository for InMemoryQuizRecordRepository {
    async fn append(&self, record: &QuizRecord) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut r = record.clone();
        r.id = id;
        self.records.lock().unwrap().push(r);
        Ok(id)
    }
}

#[derive(Default)]
pub struct InMemoryWrongItemRepository {
    wrongs: Mutex<HashMap<(i64, i64), WrongItem>>,
    next_id: AtomicI64,
}

#[async_trait]
impl WrongItemRepository for InMemoryWrongItemRepository {
    async fn find(&self, user_id: i64, question_id: i64) -> domain::Result<Option<WrongItem>> {
        Ok(self
            .wrongs
            .lock()
            .unwrap()
            .get(&(user_id, question_id))
            .cloned())
    }

    async fn record_mistake(
        &self,
        user_id: i64,
        question_id: i64,
        ts: DateTime<Utc>,
    ) -> domain::Result<WrongItem> {
        let mut map = self.wrongs.lock().unwrap();
        let entry = map.entry((user_id, question_id));
        let item = match entry {
            std::collections::hash_map::Entry::Occupied(mut o) => {
                let w = o.get_mut();
                w.record_mistake(ts);
                w.clone()
            }
            std::collections::hash_map::Entry::Vacant(v) => {
                let w = WrongItem {
                    id: take_id(&self.next_id),
                    user_id,
                    question_id,
                    times: 1,
                    mastered: false,
                    updated_at: ts,
                };
                v.insert(w.clone());
                w
            }
        };
        Ok(item)
    }

    async fn mark_mastered(
        &self,
        user_id: i64,
        question_id: i64,
        ts: DateTime<Utc>,
    ) -> domain::Result<bool> {
        let mut map = self.wrongs.lock().unwrap();
        match map.get_mut(&(user_id, question_id)) {
            Some(w) => {
                w.mark_mastered(ts);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn unmark_mastered(
        &self,
        user_id: i64,
        question_id: i64,
        ts: DateTime<Utc>,
    ) -> domain::Result<bool> {
        let mut map = self.wrongs.lock().unwrap();
        match map.get_mut(&(user_id, question_id)) {
            Some(w) => {
                w.unmark_mastered(ts);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn list_unmastered(&self, user_id: i64) -> domain::Result<Vec<WrongItem>> {
        let mut list: Vec<WrongItem> = self
            .wrongs
            .lock()
            .unwrap()
            .values()
            .filter(|w| w.user_id == user_id && !w.mastered)
            .cloned()
            .collect();
        list.sort_by_key(|w| w.updated_at);
        Ok(list)
    }

    async fn stats(&self, user_id: i64, week_ago: DateTime<Utc>) -> domain::Result<WrongStats> {
        let map = self.wrongs.lock().unwrap();
        let mut stats = WrongStats {
            total: 0,
            weekly_new: 0,
            mastered: 0,
        };
        for w in map.values() {
            if w.user_id != user_id {
                continue;
            }
            stats.total += 1;
            if w.mastered {
                stats.mastered += 1;
            }
            if w.updated_at >= week_ago {
                stats.weekly_new += 1;
            }
        }
        Ok(stats)
    }
}

#[derive(Default)]
pub struct InMemoryPaperRepository {
    papers: Mutex<HashMap<i64, Paper>>,
    next_id: AtomicI64,
}

#[async_trait]
impl PaperRepository for InMemoryPaperRepository {
    async fn insert(&self, paper: &Paper) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut p = paper.clone();
        p.id = id;
        self.papers.lock().unwrap().insert(id, p);
        Ok(id)
    }

    async fn find_by_id_and_user(&self, id: i64, user_id: i64) -> domain::Result<Option<Paper>> {
        Ok(self
            .papers
            .lock()
            .unwrap()
            .get(&id)
            .filter(|p| p.user_id == user_id)
            .cloned())
    }

    async fn submit(&self, paper: &Paper) -> domain::Result<()> {
        self.papers.lock().unwrap().insert(paper.id, paper.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryEventStore {
    events: Mutex<Vec<Event>>,
    next_id: AtomicI64,
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    async fn append(&self, event: &Event) -> domain::Result<i64> {
        let id = take_id(&self.next_id);
        let mut e = event.clone();
        e.id = id;
        self.events.lock().unwrap().push(e);
        Ok(id)
    }

    async fn list_by_user(&self, user_id: i64, limit: u32) -> domain::Result<Vec<Event>> {
        let events = self.events.lock().unwrap();
        let mut list: Vec<Event> = events
            .iter()
            .filter(|e| e.user_id == user_id)
            .rev()
            .take(limit as usize)
            .cloned()
            .collect();
        list.reverse(); // 时间升序返回，调用方按需反转
        Ok(list)
    }
}

/// 测试辅助：向内存 item 仓储插入一个节点，返回落库后的 id。
pub async fn insert_item(
    repo: &InMemoryItemRepository,
    workspace_id: i64,
    name: &str,
    kind: ItemKind,
) -> i64 {
    let item = Item {
        id: 0,
        workspace_id,
        parent_id: None,
        kind,
        name: name.to_owned(),
        content: None,
        created_by: domain::space::Creator::User,
        created_at: now(),
        updated_at: now(),
    };
    repo.insert(&item).await.unwrap()
}

/// 测试替身哈希：hash 为前缀明文，verify 反向比对（非生产算法）。
#[derive(Default)]
pub struct TestPasswordHasher;

impl PasswordHasher for TestPasswordHasher {
    fn hash(&self, plain: &str) -> domain::Result<String> {
        Ok(format!("test-hash:{plain}"))
    }

    fn verify(&self, plain: &str, hash: &str) -> bool {
        hash == format!("test-hash:{plain}")
    }
}

/// 测试替身凭证签发：usr_ 前缀 + 自增序号，序列确定便于断言。
#[derive(Default)]
pub struct TestCredentialIssuer {
    next: AtomicI64,
}

impl CredentialIssuer for TestCredentialIssuer {
    fn issue(&self) -> String {
        format!("usr_{:032x}", take_id(&self.next))
    }
}
