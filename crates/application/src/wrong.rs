//! 错题本用例：ListWrong / RedoWrong / MarkMastered。
//!
//! 错题由刷题/模考答错自动归集；重做只针对单题，不产生新的刷题会话；
//! 重做答对不自动清除错题，掌握只能由用户显式标记。
//! 列表对外以 `WrongListItem` 返回（错题记录 + 题目简述），客户端直接渲染题干。

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::ports::{QuestionRepository, WrongItemRepository};
use domain::practice::{WrongItem, WrongStats};
use serde::{Deserialize, Serialize};

use crate::quiz::QuestionBrief;

/// 错题列表项：错题记录 + 题目简述（不含答案与解析）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrongListItem {
    pub wrong: WrongItem,
    pub question: QuestionBrief,
}

pub struct WrongService<B, Q>
where
    B: WrongItemRepository,
    Q: QuestionRepository,
{
    wrongs: Arc<B>,
    questions: Arc<Q>,
}

impl<B, Q> WrongService<B, Q>
where
    B: WrongItemRepository,
    Q: QuestionRepository,
{
    pub fn new(wrongs: Arc<B>, questions: Arc<Q>) -> Self {
        Self { wrongs, questions }
    }

    /// ListWrong：未掌握错题列表（按更新时间升序，带题目简述）。
    pub async fn list(&self, user_id: i64) -> Result<Vec<WrongListItem>> {
        let wrongs = self.wrongs.list_unmastered(user_id).await?;
        let ids: Vec<i64> = wrongs.iter().map(|w| w.question_id).collect();
        let questions = self.questions.find_by_ids(&ids, user_id).await?;
        let by_id: HashMap<i64, QuestionBrief> = questions
            .into_iter()
            .map(|q| (q.id, QuestionBrief::from(q)))
            .collect();
        // 题目已删除的错题记录不展示（保留库中记录，掌握状态不受影响）。
        Ok(wrongs
            .into_iter()
            .filter_map(|wrong| {
                by_id
                    .get(&wrong.question_id)
                    .cloned()
                    .map(|question| WrongListItem { wrong, question })
            })
            .collect())
    }

    /// RedoWrong：重做单道错题，返回题目视图（不含答案与解析）。
    pub async fn redo(&self, user_id: i64, question_id: i64) -> Result<QuestionBrief> {
        if self.wrongs.find(user_id, question_id).await?.is_none() {
            return Err(Error::NotFound("错题不存在".to_owned()));
        }
        let q = self
            .questions
            .find_by_ids(&[question_id], user_id)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound("题目不存在".to_owned()))?;
        Ok(QuestionBrief::from(q))
    }

    /// WrongStats：错题统计卡片（累计 / 近 7 天新增 / 已掌握）。
    pub async fn stats(&self, user_id: i64) -> Result<WrongStats> {
        let week_ago = Utc::now() - chrono::Duration::days(7);
        self.wrongs.stats(user_id, week_ago).await
    }

    /// MarkMastered：显式标记掌握；错题不存在报 NotFound。
    pub async fn mark_mastered(&self, user_id: i64, question_id: i64) -> Result<()> {
        let hit = self
            .wrongs
            .mark_mastered(user_id, question_id, Utc::now())
            .await?;
        if hit {
            Ok(())
        } else {
            Err(Error::NotFound("错题不存在".to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{InMemoryQuestionRepository, InMemoryWrongItemRepository};
    use domain::practice::{Answer, Question, QuestionType};

    async fn svc() -> (
        WrongService<InMemoryWrongItemRepository, InMemoryQuestionRepository>,
        i64,
    ) {
        let wrongs = Arc::new(InMemoryWrongItemRepository::default());
        let questions = Arc::new(InMemoryQuestionRepository::default());
        let q = Question {
            id: 0,
            workspace_id: 1,
            source_item_id: 10,
            qtype: QuestionType::Single,
            stem: "1+1=?".into(),
            options: vec!["1".into(), "2".into()],
            answer: Answer::Single(1),
            explanation: None,
            created_at: Utc::now(),
        };
        let ids = questions.insert_many(&[q]).await.unwrap();
        wrongs.record_mistake(1, ids[0], Utc::now()).await.unwrap();
        (WrongService::new(wrongs, questions), ids[0])
    }

    #[tokio::test]
    async fn list_returns_only_unmastered_with_question_brief() {
        let (s, qid) = svc().await;
        let list = s.list(1).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].wrong.question_id, qid);
        assert_eq!(list[0].question.stem, "1+1=?");
        // 简述不含答案。
        assert!(
            serde_json::to_value(&list[0].question)
                .unwrap()
                .get("answer")
                .is_none()
        );
        s.mark_mastered(1, qid).await.unwrap();
        assert!(s.list(1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn redo_returns_brief_and_requires_wrong_item() {
        let (s, qid) = svc().await;
        let brief = s.redo(1, qid).await.unwrap();
        assert_eq!(brief.stem, "1+1=?");
        assert!(
            serde_json::to_value(&brief)
                .unwrap()
                .get("answer")
                .is_none()
        );
        // 不是错题 → NotFound。
        assert!(matches!(s.redo(1, 999).await, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn stats_counts_total_weekly_and_mastered() {
        let (s, qid) = svc().await;
        let other = s.wrongs.record_mistake(1, 777, Utc::now()).await.unwrap();
        assert!(!other.mastered);
        // 已掌握 1 道；weekly_new 随 now 计入（updated_at 均在 7 天窗口内）。
        s.mark_mastered(1, qid).await.unwrap();
        let stats = s.stats(1).await.unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.mastered, 1);
        assert_eq!(stats.weekly_new, 2);
        // 隔离：他人错题不计入。
        s.wrongs.record_mistake(2, qid, Utc::now()).await.unwrap();
        let stats = s.stats(1).await.unwrap();
        assert_eq!(stats.total, 2);
        // 早期错题不计入 weekly_new。
        let stale = Utc::now() - chrono::Duration::days(30);
        let _ = s.wrongs.record_mistake(1, 888, stale).await.unwrap();
        let stats = s.stats(1).await.unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.weekly_new, 2);
    }

    #[tokio::test]
    async fn mark_mastered_is_isolated_and_idempotent() {
        let (s, qid) = svc().await;
        s.mark_mastered(1, qid).await.unwrap();
        s.mark_mastered(1, qid).await.unwrap(); // 重复标记幂等
        assert!(matches!(
            s.mark_mastered(2, qid).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn redo_does_not_auto_master() {
        let (s, qid) = svc().await;
        let _ = s.redo(1, qid).await.unwrap();
        // 重做答对不自动清除错题：WrongItem 无该行为路径，
        // 掌握只能由 mark_mastered 显式翻转。
        assert!(!s.wrongs.find(1, qid).await.unwrap().unwrap().mastered);
    }
}
