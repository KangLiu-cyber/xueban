//! 刷题用例：DrawQuestions / SubmitAnswer。
//!
//! 抽题响应只返回题目视图（不含答案与解析），防客户端作弊；
//! 提交作答后返回判定、正确答案与解析。每次作答都落 answer 事件，
//! 答错额外落 wrong 事件并更新错题本。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::{
    EventStore, ItemRepository, QuestionRepository, QuizRecordRepository, WorkspaceRepository,
    WrongItemRepository,
};
use domain::practice::{Answer, Chosen, Question, QuestionType, QuizRecord};
use serde::{Deserialize, Serialize};

/// 题目视图：抽题响应不携带答案与解析。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionBrief {
    pub id: i64,
    pub source_item_id: i64,
    pub qtype: QuestionType,
    pub stem: String,
    pub options: Vec<String>,
}

impl From<Question> for QuestionBrief {
    fn from(q: Question) -> Self {
        Self {
            id: q.id,
            source_item_id: q.source_item_id,
            qtype: q.qtype,
            stem: q.stem,
            options: q.options,
        }
    }
}

/// 作答结果：判定 + 正确答案 + 解析（提交后返回，供答题页展示）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerOutcome {
    pub is_correct: bool,
    pub answer: Answer,
    pub explanation: Option<String>,
}

pub struct QuizService<W, I, Q, R, B, E>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    R: QuizRecordRepository + ?Sized,
    B: WrongItemRepository + ?Sized,
    E: EventStore + ?Sized,
{
    workspaces: Arc<W>,
    items: Arc<I>,
    questions: Arc<Q>,
    records: Arc<R>,
    wrongs: Arc<B>,
    events: Arc<E>,
}

impl<W, I, Q, R, B, E> QuizService<W, I, Q, R, B, E>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    R: QuizRecordRepository + ?Sized,
    B: WrongItemRepository + ?Sized,
    E: EventStore + ?Sized,
{
    pub fn new(
        workspaces: Arc<W>,
        items: Arc<I>,
        questions: Arc<Q>,
        records: Arc<R>,
        wrongs: Arc<B>,
        events: Arc<E>,
    ) -> Self {
        Self {
            workspaces,
            items,
            questions,
            records,
            wrongs,
            events,
        }
    }

    /// DrawQuestions：按集抽题。`scope` 为该集的节点 id（可选）；
    /// 未给 scope 则在整个空间题库中抽取。
    pub async fn draw(
        &self,
        user_id: i64,
        workspace_id: i64,
        scope: Option<i64>,
        count: u32,
    ) -> Result<Vec<QuestionBrief>> {
        self.workspaces
            .find_by_id_and_user(workspace_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("备考空间不存在".to_owned()))?;
        if let Some(scope) = scope {
            self.items
                .find_by_id(scope, user_id)
                .await?
                .ok_or_else(|| Error::NotFound("刷题范围不存在".to_owned()))?;
        }
        let source = scope.map(|s| vec![s]);
        let qs = self
            .questions
            .draw(workspace_id, source.as_deref().unwrap_or(&[]), &[], count)
            .await?;
        Ok(qs.into_iter().map(QuestionBrief::from).collect())
    }

    /// SubmitAnswer：取题 → 判分 → 追加作答记录 → 答错记错题 + wrong 事件。
    pub async fn submit(
        &self,
        user_id: i64,
        question_id: i64,
        chosen: Chosen,
        scope: Option<String>,
    ) -> Result<AnswerOutcome> {
        let question = self
            .questions
            .find_by_ids(&[question_id], user_id)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound("题目不存在".to_owned()))?;
        // 题目不存 user_id：归属经所属空间校验。
        self.workspaces
            .find_by_id_and_user(question.workspace_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("题目不存在".to_owned()))?;
        let judgment = question.judge(&chosen)?;
        let now = Utc::now();
        let record = QuizRecord {
            id: 0,
            user_id,
            question_id,
            scope,
            chosen: Some(chosen),
            is_correct: judgment.is_correct,
            created_at: now,
        };
        self.records.append(&record).await?;
        // 审计：每次作答都落事件。
        self.events
            .append(&Event {
                id: 0,
                user_id,
                workspace_id: Some(question.workspace_id),
                item_id: Some(question.source_item_id),
                action: EventAction::Answer,
                payload: Some(
                    serde_json::json!({
                        "question_id": question_id,
                        "is_correct": judgment.is_correct,
                    })
                    .to_string(),
                ),
                created_at: now,
            })
            .await?;
        if !judgment.is_correct {
            self.wrongs
                .record_mistake(user_id, question_id, now)
                .await?;
            self.events
                .append(&Event {
                    id: 0,
                    user_id,
                    workspace_id: Some(question.workspace_id),
                    item_id: Some(question.source_item_id),
                    action: EventAction::Wrong,
                    payload: Some(
                        serde_json::json!({ "question_id": question_id, "times_after": 0 })
                            .to_string(),
                    ),
                    created_at: now,
                })
                .await?;
        }
        Ok(AnswerOutcome {
            is_correct: judgment.is_correct,
            answer: question.answer,
            explanation: question.explanation,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryEventStore, InMemoryItemRepository, InMemoryQuestionRepository,
        InMemoryQuizRecordRepository, InMemoryWorkspaceRepository, InMemoryWrongItemRepository,
    };
    use domain::space::{ItemKind, Workspace};

    fn workspace(user_id: i64) -> Workspace {
        Workspace {
            id: 0,
            user_id,
            name: "备考".into(),
            exam_goal: "目标".into(),
            exam_date: None,
            created_at: Utc::now(),
        }
    }

    struct Ctx {
        svc: QuizService<
            InMemoryWorkspaceRepository,
            InMemoryItemRepository,
            InMemoryQuestionRepository,
            InMemoryQuizRecordRepository,
            InMemoryWrongItemRepository,
            InMemoryEventStore,
        >,
        ws_id: i64,
        item_id: i64,
        q_ids: Vec<i64>,
    }

    async fn ctx(user_id: i64) -> Ctx {
        let ws_repo = Arc::new(InMemoryWorkspaceRepository::default());
        let item_repo = Arc::new(InMemoryItemRepository::default());
        let q_repo = Arc::new(InMemoryQuestionRepository::default());
        let ws = ws_repo.insert(&workspace(user_id)).await.unwrap();
        let item = crate::inmem::insert_item(&item_repo, ws, "第1集", ItemKind::Dir).await;
        let questions = vec![
            Question {
                id: 0,
                workspace_id: ws,
                source_item_id: item,
                qtype: QuestionType::Single,
                stem: "1+1=?".into(),
                options: vec!["1".into(), "2".into()],
                answer: Answer::Single(1),
                explanation: Some("算术".into()),
                created_at: Utc::now(),
            },
            Question {
                id: 0,
                workspace_id: ws,
                source_item_id: item,
                qtype: QuestionType::Judge,
                stem: "地球是圆的".into(),
                options: vec![],
                answer: Answer::Judge(true),
                explanation: None,
                created_at: Utc::now(),
            },
        ];
        let ids = q_repo.insert_many(&questions).await.unwrap();
        let svc = QuizService::new(
            ws_repo,
            item_repo,
            q_repo,
            Arc::new(InMemoryQuizRecordRepository::default()),
            Arc::new(InMemoryWrongItemRepository::default()),
            Arc::new(InMemoryEventStore::default()),
        );
        Ctx {
            svc,
            ws_id: ws,
            item_id: item,
            q_ids: ids,
        }
    }

    #[tokio::test]
    async fn draw_scoped_returns_briefs_without_answer() {
        let c = ctx(1).await;
        let qs = c.svc.draw(1, c.ws_id, Some(c.item_id), 10).await.unwrap();
        assert_eq!(qs.len(), 2);
        // 抽题响应不含答案与解析。
        assert_eq!(qs[0].options.len(), 2);
        let ser = serde_json::to_value(&qs[0]).unwrap();
        assert!(ser.get("answer").is_none());
        assert!(ser.get("explanation").is_none());
    }

    #[tokio::test]
    async fn draw_isolates_user_and_scope() {
        let c = ctx(1).await;
        assert!(c.svc.draw(2, c.ws_id, None, 10).await.is_err());
        assert!(c.svc.draw(1, c.ws_id, Some(999), 10).await.is_err());
    }

    #[tokio::test]
    async fn submit_correct_appends_record_and_answer_event() {
        let c = ctx(1).await;
        let out = c
            .svc
            .submit(1, c.q_ids[0], Chosen::Single(1), Some("第1集".into()))
            .await
            .unwrap();
        assert!(out.is_correct);
        assert_eq!(out.answer, Answer::Single(1));
        let events = c.svc.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::Answer);
        // 答对不产生错题。
        assert!(c.svc.wrongs.find(1, c.q_ids[0]).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn submit_wrong_accumulates_wrong_item_and_events() {
        let c = ctx(1).await;
        let out = c
            .svc
            .submit(1, c.q_ids[0], Chosen::Single(0), None)
            .await
            .unwrap();
        assert!(!out.is_correct);
        let w = c.svc.wrongs.find(1, c.q_ids[0]).await.unwrap().unwrap();
        assert_eq!(w.times, 1);
        assert!(!w.mastered);
        // 再错一次累计 times。
        c.svc
            .submit(1, c.q_ids[0], Chosen::Single(0), None)
            .await
            .unwrap();
        let w = c.svc.wrongs.find(1, c.q_ids[0]).await.unwrap().unwrap();
        assert_eq!(w.times, 2);
        // answer + wrong 事件各一条（两次作答）。
        let events = c.svc.events.list_by_user(1, 10).await.unwrap();
        let wrongs: Vec<_> = events
            .iter()
            .filter(|e| e.action == EventAction::Wrong)
            .collect();
        assert_eq!(wrongs.len(), 2);
    }

    #[tokio::test]
    async fn submit_rejects_foreign_question() {
        // 他人空间中的题目不可作答：向同一仓储注入用户 2 的空间与题目。
        let c = ctx(1).await;
        let ws2 = c.svc.workspaces.insert(&workspace(2)).await.unwrap();
        let foreign = Question {
            id: 0,
            workspace_id: ws2,
            source_item_id: c.item_id,
            qtype: QuestionType::Single,
            stem: "他题".into(),
            options: vec![],
            answer: Answer::Single(0),
            explanation: None,
            created_at: Utc::now(),
        };
        let qid = c.svc.questions.insert_many(&[foreign]).await.unwrap()[0];
        assert!(matches!(
            c.svc.submit(1, qid, Chosen::Single(0), None).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn submit_rejects_unknown_question_and_mismatched_type() {
        let c = ctx(1).await;
        assert!(matches!(
            c.svc.submit(1, 999, Chosen::Single(0), None).await,
            Err(Error::NotFound(_))
        ));
        // judge 题用 single 作答 → Invalid。
        assert!(matches!(
            c.svc.submit(1, c.q_ids[1], Chosen::Single(0), None).await,
            Err(Error::Invalid(_))
        ));
    }
}
