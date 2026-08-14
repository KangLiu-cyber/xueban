//! 错题本用例：ListWrong / RedoWrong / MarkMastered。
//!
//! 错题由刷题/模考答错自动归集；重做只针对单题，不产生新的刷题会话；
//! 重做答对不自动清除错题，掌握只能由用户显式标记。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::ports::{QuestionRepository, WrongItemRepository};
use domain::practice::WrongItem;

use crate::quiz::QuestionBrief;

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

    /// ListWrong：未掌握错题列表。
    pub async fn list(&self, user_id: i64) -> Result<Vec<WrongItem>> {
        self.wrongs.list_unmastered(user_id).await
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
    async fn list_returns_only_unmastered() {
        let (s, qid) = svc().await;
        assert_eq!(s.list(1).await.unwrap().len(), 1);
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
