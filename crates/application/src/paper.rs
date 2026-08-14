//! 组卷模考用例：AssemblePaper / ReadPaper / SubmitPaper。
//!
//! 组卷是"筛选条件 + 题目快照"：抽题后 `question_ids` 冻结；
//! 抽题不足 count 时按规则补齐（先放宽题型，再放宽来源，去重）。
//! 交卷判分逐题调用领域纯函数，答错回流错题本并落 wrong 事件。

use std::sync::Arc;

use chrono::Utc;
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::{
    EventStore, ItemRepository, PaperRepository, QuestionRepository, WorkspaceRepository,
    WrongItemRepository,
};
use domain::practice::{Chosen, Paper, PaperConfig, PaperResult, Question};
use serde::{Deserialize, Serialize};

/// 交卷入参：题号 → 所选答案。缺答的题按答错计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperAnswer {
    pub question_id: i64,
    pub chosen: Chosen,
}

pub struct PaperService<W, I, Q, P, B, E>
where
    W: WorkspaceRepository,
    I: ItemRepository,
    Q: QuestionRepository,
    P: PaperRepository,
    B: WrongItemRepository,
    E: EventStore,
{
    workspaces: Arc<W>,
    items: Arc<I>,
    questions: Arc<Q>,
    papers: Arc<P>,
    wrongs: Arc<B>,
    events: Arc<E>,
}

impl<W, I, Q, P, B, E> PaperService<W, I, Q, P, B, E>
where
    W: WorkspaceRepository,
    I: ItemRepository,
    Q: QuestionRepository,
    P: PaperRepository,
    B: WrongItemRepository,
    E: EventStore,
{
    pub fn new(
        workspaces: Arc<W>,
        items: Arc<I>,
        questions: Arc<Q>,
        papers: Arc<P>,
        wrongs: Arc<B>,
        events: Arc<E>,
    ) -> Self {
        Self {
            workspaces,
            items,
            questions,
            papers,
            wrongs,
            events,
        }
    }

    /// AssemblePaper：校验归属 → 抽题（含补齐）→ 冻结快照。
    pub async fn assemble(
        &self,
        user_id: i64,
        workspace_id: i64,
        name: Option<String>,
        config: PaperConfig,
    ) -> Result<Paper> {
        self.workspaces
            .find_by_id_and_user(workspace_id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("备考空间不存在".to_owned()))?;
        if let Some(ids) = &config.source_item_ids {
            for id in ids {
                self.items
                    .find_by_id(*id, user_id)
                    .await?
                    .ok_or_else(|| Error::NotFound("组卷来源不存在".to_owned()))?;
            }
        }
        let sources = config.source_item_ids.clone().unwrap_or_default();
        let qtypes = config.question_types.clone().unwrap_or_default();
        let mut drawn = self
            .questions
            .draw(workspace_id, &sources, &qtypes, config.count)
            .await?;
        if (drawn.len() as u32) < config.count {
            // 补齐：放宽题型，仍按来源。
            let extra = self
                .questions
                .draw(
                    workspace_id,
                    &sources,
                    &[],
                    config.count - drawn.len() as u32,
                )
                .await?;
            push_unique(&mut drawn, extra);
        }
        if (drawn.len() as u32) < config.count {
            // 补齐：放宽来源，全空间取题（仍去重）。
            let extra = self
                .questions
                .draw(workspace_id, &[], &[], config.count - drawn.len() as u32)
                .await?;
            push_unique(&mut drawn, extra);
        }
        let question_ids: Vec<i64> = drawn.into_iter().map(|q| q.id).collect();
        let paper = Paper {
            id: 0,
            user_id,
            workspace_id,
            name,
            config,
            question_ids,
            result: None,
            created_at: Utc::now(),
        };
        let id = self.papers.insert(&paper).await?;
        let mut paper = paper;
        paper.id = id;
        Ok(paper)
    }

    /// ReadPaper：读试卷（含题目快照与结果）。
    pub async fn read(&self, user_id: i64, id: i64) -> Result<Paper> {
        self.papers
            .find_by_id_and_user(id, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("试卷不存在".to_owned()))
    }

    /// SubmitPaper：交卷判分。每题 1 分，缺答计错；答错回流错题本。
    pub async fn submit(
        &self,
        user_id: i64,
        paper_id: i64,
        answers: Vec<PaperAnswer>,
        duration_secs: u32,
    ) -> Result<PaperResult> {
        let mut paper = self.read(user_id, paper_id).await?;
        if paper.result.is_some() {
            return Err(Error::Conflict("试卷已交卷".to_owned()));
        }
        let questions = self
            .questions
            .find_by_ids(&paper.question_ids, user_id)
            .await?;
        let answered: std::collections::HashMap<i64, &Chosen> =
            answers.iter().map(|a| (a.question_id, &a.chosen)).collect();
        let now = Utc::now();
        let mut correct = 0u32;
        let mut wrong_ids = Vec::new();
        for q in &questions {
            match answered.get(&q.id) {
                Some(chosen) if q.judge(chosen)?.is_correct => correct += 1,
                Some(_) | None => {
                    wrong_ids.push(q.id);
                }
            }
        }
        // 答错回流错题本。
        for q in questions.iter().filter(|q| wrong_ids.contains(&q.id)) {
            self.wrongs.record_mistake(user_id, q.id, now).await?;
            self.events
                .append(&Event {
                    id: 0,
                    user_id,
                    workspace_id: Some(paper.workspace_id),
                    item_id: Some(q.source_item_id),
                    action: EventAction::Wrong,
                    payload: Some(
                        serde_json::json!({
                            "question_id": q.id,
                            "paper_id": paper_id,
                        })
                        .to_string(),
                    ),
                    created_at: now,
                })
                .await?;
        }
        let total = paper.question_ids.len() as u32;
        let result = PaperResult {
            score: correct,
            correct,
            total,
            duration_secs,
        };
        paper.result = Some(result);
        self.papers.submit(&paper).await?;
        Ok(result)
    }
}

fn push_unique(drawn: &mut Vec<Question>, extra: Vec<Question>) {
    for q in extra {
        if !drawn.iter().any(|d| d.id == q.id) {
            drawn.push(q);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::{
        InMemoryEventStore, InMemoryItemRepository, InMemoryPaperRepository,
        InMemoryQuestionRepository, InMemoryWorkspaceRepository, InMemoryWrongItemRepository,
    };
    use domain::practice::{Answer, QuestionType};
    use domain::space::{ItemKind, Workspace};

    struct Ctx {
        svc: PaperService<
            InMemoryWorkspaceRepository,
            InMemoryItemRepository,
            InMemoryQuestionRepository,
            InMemoryPaperRepository,
            InMemoryWrongItemRepository,
            InMemoryEventStore,
        >,
        ws_id: i64,
        item_id: i64,
        q_ids: Vec<i64>,
    }

    async fn ctx() -> Ctx {
        let ws_repo = Arc::new(InMemoryWorkspaceRepository::default());
        let item_repo = Arc::new(InMemoryItemRepository::default());
        let q_repo = Arc::new(InMemoryQuestionRepository::default());
        let ws = Workspace {
            id: 0,
            user_id: 1,
            name: "备考".into(),
            exam_goal: "目标".into(),
            exam_date: None,
            created_at: Utc::now(),
        };
        let ws_id = ws_repo.insert(&ws).await.unwrap();
        let item_id = crate::inmem::insert_item(&item_repo, ws_id, "第1集", ItemKind::Note).await;
        let questions: Vec<Question> = (0..3)
            .map(|i| Question {
                id: 0,
                workspace_id: ws_id,
                source_item_id: item_id,
                qtype: QuestionType::Single,
                stem: format!("题{i}"),
                options: vec!["A".into(), "B".into()],
                answer: Answer::Single(1),
                explanation: None,
                created_at: Utc::now(),
            })
            .collect();
        let q_ids = q_repo.insert_many(&questions).await.unwrap();
        let svc = PaperService::new(
            ws_repo,
            item_repo,
            q_repo,
            Arc::new(InMemoryPaperRepository::default()),
            Arc::new(InMemoryWrongItemRepository::default()),
            Arc::new(InMemoryEventStore::default()),
        );
        Ctx {
            svc,
            ws_id,
            item_id,
            q_ids,
        }
    }

    fn config(count: u32) -> PaperConfig {
        PaperConfig {
            scope: Some("第1集".into()),
            question_types: None,
            source_item_ids: None,
            count,
        }
    }

    #[tokio::test]
    async fn assemble_freezes_question_snapshot() {
        let c = ctx().await;
        let p = c
            .svc
            .assemble(1, c.ws_id, Some("模考一".into()), config(3))
            .await
            .unwrap();
        assert_eq!(p.question_ids.len(), 3);
        assert_eq!(p.question_ids, c.q_ids);
        assert!(p.result.is_none());
        // 题库变化不影响已组试卷快照。
        c.svc
            .questions
            .insert_many(&[Question {
                id: 0,
                workspace_id: c.ws_id,
                source_item_id: c.item_id,
                qtype: QuestionType::Single,
                stem: "新题".into(),
                options: vec![],
                answer: Answer::Single(0),
                explanation: None,
                created_at: Utc::now(),
            }])
            .await
            .unwrap();
        let again = c.svc.read(1, p.id).await.unwrap();
        assert_eq!(again.question_ids.len(), 3);
    }

    #[tokio::test]
    async fn assemble_top_up_without_source_when_short() {
        let c = ctx().await;
        // count 超过题库总量 → 全部取到（不足不报错）。
        let p = c.svc.assemble(1, c.ws_id, None, config(10)).await.unwrap();
        assert_eq!(p.question_ids.len(), 3);
    }

    #[tokio::test]
    async fn assemble_isolates_user_and_source() {
        let c = ctx().await;
        assert!(c.svc.assemble(2, c.ws_id, None, config(1)).await.is_err());
        let bad = PaperConfig {
            source_item_ids: Some(vec![999]),
            ..config(1)
        };
        assert!(c.svc.assemble(1, c.ws_id, None, bad).await.is_err());
    }

    #[tokio::test]
    async fn submit_scores_and_refills_wrong_book() {
        let c = ctx().await;
        let p = c.svc.assemble(1, c.ws_id, None, config(3)).await.unwrap();
        let answers = vec![
            PaperAnswer {
                question_id: c.q_ids[0],
                chosen: Chosen::Single(1),
            },
            PaperAnswer {
                question_id: c.q_ids[1],
                chosen: Chosen::Single(0), // 答错
            },
            // q_ids[2] 缺答 → 计错
        ];
        let r = c.svc.submit(1, p.id, answers, 300).await.unwrap();
        assert_eq!(r.correct, 1);
        assert_eq!(r.score, 1);
        assert_eq!(r.total, 3);
        assert_eq!(r.duration_secs, 300);
        assert!((r.accuracy() - 1.0 / 3.0).abs() < 1e-9);
        // 错题回流。
        assert!(c.svc.wrongs.find(1, c.q_ids[1]).await.unwrap().is_some());
        assert!(c.svc.wrongs.find(1, c.q_ids[2]).await.unwrap().is_some());
        // 两道 wrong 事件。
        let events = c.svc.events.list_by_user(1, 10).await.unwrap();
        let wrong_events = events
            .iter()
            .filter(|e| e.action == EventAction::Wrong)
            .count();
        assert_eq!(wrong_events, 2);
    }

    #[tokio::test]
    async fn submit_rejects_second_time_and_other_user() {
        let c = ctx().await;
        let p = c.svc.assemble(1, c.ws_id, None, config(1)).await.unwrap();
        c.svc.submit(1, p.id, vec![], 10).await.unwrap();
        assert!(matches!(
            c.svc.submit(1, p.id, vec![], 10).await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            c.svc.submit(2, p.id, vec![], 10).await,
            Err(Error::NotFound(_))
        ));
    }
}
