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
    AttachmentRepository, EventStore, ItemRepository, QuestionRepository, QuizRecordRepository,
    WorkspaceRepository, WrongItemRepository,
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

pub struct QuizService<W, I, Q, R, B, E, A>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    R: QuizRecordRepository + ?Sized,
    B: WrongItemRepository + ?Sized,
    E: EventStore + ?Sized,
    A: AttachmentRepository + ?Sized,
{
    workspaces: Arc<W>,
    items: Arc<I>,
    questions: Arc<Q>,
    records: Arc<R>,
    wrongs: Arc<B>,
    events: Arc<E>,
    attachments: Arc<A>,
}

impl<W, I, Q, R, B, E, A> QuizService<W, I, Q, R, B, E, A>
where
    W: WorkspaceRepository + ?Sized,
    I: ItemRepository + ?Sized,
    Q: QuestionRepository + ?Sized,
    R: QuizRecordRepository + ?Sized,
    B: WrongItemRepository + ?Sized,
    E: EventStore + ?Sized,
    A: AttachmentRepository + ?Sized,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspaces: Arc<W>,
        items: Arc<I>,
        questions: Arc<Q>,
        records: Arc<R>,
        wrongs: Arc<B>,
        events: Arc<E>,
        attachments: Arc<A>,
    ) -> Self {
        Self {
            workspaces,
            items,
            questions,
            records,
            wrongs,
            events,
            attachments,
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
            .draw(
                workspace_id,
                user_id,
                source.as_deref().unwrap_or(&[]),
                &[],
                count,
            )
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
            let item = self
                .wrongs
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
                        serde_json::json!({
                            "question_id": question_id,
                            "times_after": item.times,
                        })
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

    /// SubmitVideoAnswer：视频题作答——用户上传训练视频（附件）与训练想法，
    /// 不判对错、不进错题本，仅校验归属与题型后落 `video_submit` 事件，
    /// 供复盘 Agent 经 get_events 读取（附件 id + 训练想法 + 题源）。
    pub async fn submit_video(
        &self,
        user_id: i64,
        question_id: i64,
        attachment_ids: Vec<i64>,
        note: Option<String>,
    ) -> Result<()> {
        let question = self
            .questions
            .find_by_ids(&[question_id], user_id)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound("题目不存在".to_owned()))?;
        self.workspaces
            .find_by_id_and_user(question.workspace_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("题目不存在".to_owned()))?;
        if question.qtype != QuestionType::Video {
            return Err(Error::Invalid("该题不是视频作答题".to_owned()));
        }
        if attachment_ids.is_empty() {
            return Err(Error::Invalid("请先上传训练视频".to_owned()));
        }
        // 附件归属校验：每个附件都必须属于当前用户（SQL join 防线 + 显式未命中 → NotFound）。
        for id in &attachment_ids {
            self.attachments
                .find_by_id(*id, user_id)
                .await?
                .ok_or_else(|| Error::NotFound("训练视频附件不存在".to_owned()))?;
        }
        let now = Utc::now();
        self.events
            .append(&Event {
                id: 0,
                user_id,
                workspace_id: Some(question.workspace_id),
                item_id: Some(question.source_item_id),
                action: EventAction::VideoSubmit,
                payload: Some(
                    serde_json::json!({
                        "question_id": question_id,
                        "attachment_ids": attachment_ids,
                        "note": note,
                    })
                    .to_string(),
                ),
                created_at: now,
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryAttachmentRepository, InMemoryEventStore, InMemoryItemRepository,
        InMemoryQuestionRepository, InMemoryQuizRecordRepository, InMemoryWorkspaceRepository,
        InMemoryWrongItemRepository,
    };
    use domain::space::{Attachment, ItemKind, Workspace};

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
            InMemoryAttachmentRepository,
        >,
        ws_id: i64,
        item_id: i64,
        q_ids: Vec<i64>,
    }

    async fn ctx(user_id: i64) -> Ctx {
        let ws_repo = Arc::new(InMemoryWorkspaceRepository::default());
        // item 注入 workspace 注册表（归属过滤），附件注入同一 item 仓储（附件归属校验）。
        let item_repo = Arc::new(InMemoryItemRepository::with_workspaces(ws_repo.clone()));
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
            item_repo.clone(),
            q_repo,
            Arc::new(InMemoryQuizRecordRepository::default()),
            Arc::new(InMemoryWrongItemRepository::default()),
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemoryAttachmentRepository::with_items(item_repo)),
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
        // wrong 事件 payload 携带真实累计次数（时间升序：1 次后 2 次后）。
        let times: Vec<i64> = wrongs
            .iter()
            .map(|e| {
                serde_json::from_str::<serde_json::Value>(e.payload.as_deref().unwrap()).unwrap()
                    ["times_after"]
                    .as_i64()
                    .unwrap()
            })
            .collect();
        assert_eq!(times, vec![1, 2]);
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

    #[tokio::test]
    async fn submit_video_appends_event_without_judging() {
        let c = ctx(1).await;
        // 挂一条视频题（Video 题型，无标准答案）。
        let vid = c
            .svc
            .questions
            .insert_many(&[Question {
                id: 0,
                workspace_id: c.ws_id,
                source_item_id: c.item_id,
                qtype: QuestionType::Video,
                stem: "上传正手高远球动作视频".into(),
                options: vec![],
                answer: Answer::Video,
                explanation: None,
                created_at: Utc::now(),
            }])
            .await
            .unwrap()[0];
        // 一个属于该用户的附件（挂在题目所属 item 下）。
        let att = Attachment {
            id: 0,
            item_id: c.item_id,
            filename: "训练.mp4".into(),
            mime: "video/mp4".into(),
            size_bytes: 1024,
            uuid: "u".into(),
            created_at: Utc::now(),
        };
        let att_id = c.svc.attachments.insert(&att).await.unwrap();

        c.svc
            .submit_video(1, vid, vec![att_id], Some("发力用不上腰".into()))
            .await
            .unwrap();

        // 落 video_submit 事件，payload 携带附件 id 与训练想法。
        let events = c.svc.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::VideoSubmit);
        assert_eq!(events[0].item_id, Some(c.item_id));
        let payload: serde_json::Value =
            serde_json::from_str(events[0].payload.as_deref().unwrap()).unwrap();
        assert_eq!(payload["question_id"], vid);
        assert_eq!(payload["attachment_ids"], serde_json::json!([att_id]));
        assert_eq!(payload["note"], "发力用不上腰");
        // 视频题不判分、不进错题本：无 answer 事件、无错题。
        assert!(c.svc.wrongs.find(1, vid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn submit_video_rejects_wrong_type_empty_foreign_attachment() {
        let c = ctx(1).await;
        let att = Attachment {
            id: 0,
            item_id: c.item_id,
            filename: "a.mp4".into(),
            mime: "video/mp4".into(),
            size_bytes: 1,
            uuid: "u".into(),
            created_at: Utc::now(),
        };
        let att_id = c.svc.attachments.insert(&att).await.unwrap();
        // 非视频题（single）→ Invalid。
        assert!(matches!(
            c.svc.submit_video(1, c.q_ids[0], vec![att_id], None).await,
            Err(Error::Invalid(_))
        ));
        let vid = c
            .svc
            .questions
            .insert_many(&[Question {
                id: 0,
                workspace_id: c.ws_id,
                source_item_id: c.item_id,
                qtype: QuestionType::Video,
                stem: "上传视频".into(),
                options: vec![],
                answer: Answer::Video,
                explanation: None,
                created_at: Utc::now(),
            }])
            .await
            .unwrap()[0];
        // 空附件列表 → Invalid。
        assert!(matches!(
            c.svc.submit_video(1, vid, vec![], None).await,
            Err(Error::Invalid(_))
        ));
        // 他人附件 → NotFound（附件归属校验，附件挂在用户 2 的 item 下）。
        let ws2 = c.svc.workspaces.insert(&workspace(2)).await.unwrap();
        let item2 = crate::inmem::insert_item(&c.svc.items, ws2, "别人集", ItemKind::Dir).await;
        let foreign = Attachment {
            id: 0,
            item_id: item2,
            filename: "b.mp4".into(),
            mime: "video/mp4".into(),
            size_bytes: 1,
            uuid: "v".into(),
            created_at: Utc::now(),
        };
        let foreign_id = c.svc.attachments.insert(&foreign).await.unwrap();
        assert!(matches!(
            c.svc.submit_video(1, vid, vec![foreign_id], None).await,
            Err(Error::NotFound(_))
        ));
    }
}
