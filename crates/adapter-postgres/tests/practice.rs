//! 练习与复盘仓储集成测试（env-gated）：题库 / 作答 / 错题 / 试卷 / 事件。

mod common;

use adapter_postgres::{
    PgEventStore, PgItemRepository, PgPaperRepository, PgQuestionRepository,
    PgQuizRecordRepository, PgUserRepository, PgWorkspaceRepository, PgWrongItemRepository,
};
use chrono::Utc;
use common::{insert_user, pool, setup, stamp};
use domain::event::{Event, EventAction};
use domain::ports::{
    EventStore, ItemRepository, PaperRepository, QuestionRepository, QuizRecordRepository,
    WorkspaceRepository, WrongItemRepository,
};
use domain::practice::{
    Answer, Chosen, Paper, PaperConfig, PaperResult, Question, QuestionType, QuizRecord,
};
use domain::space::{Creator, Item, ItemKind, Workspace};
use std::collections::BTreeSet;

fn new_workspace(user_id: i64) -> Workspace {
    Workspace {
        id: 0,
        user_id,
        name: format!("练习空间_{}", stamp()),
        exam_goal: "通过考试".into(),
        exam_date: None,
        created_at: Utc::now(),
    }
}

fn question(workspace_id: i64, source_item_id: i64, qtype: QuestionType) -> Question {
    Question {
        id: 0,
        workspace_id,
        source_item_id,
        qtype,
        stem: format!("题干_{}", stamp()),
        options: vec!["甲".into(), "乙".into(), "丙".into()],
        answer: match qtype {
            QuestionType::Single => Answer::Single(1),
            QuestionType::Multi => Answer::Multi(BTreeSet::from([0, 2])),
            QuestionType::Judge => Answer::Judge(true),
        },
        explanation: Some("解析".into()),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn question_insert_many_find_and_draw() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "q").await;
    let other = insert_user(&user_repo, "q2").await;
    let ws_repo = PgWorkspaceRepository::new(pool.clone());
    let ws_id = ws_repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let other_ws = ws_repo
        .insert(&new_workspace(other))
        .await
        .expect("插入空间失败");
    let repo = PgQuestionRepository::new(pool.clone());

    // questions.source_item_id 外键指向真实 items，先种两个"集"。
    let item_repo = PgItemRepository::new(pool);
    let item = |ws: i64, name: &str| Item {
        id: 0,
        workspace_id: ws,
        parent_id: None,
        kind: ItemKind::Note,
        name: format!("{name}_{}", stamp()),
        content: Some("内容".into()),
        created_by: Creator::Agent,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let mine_a = item_repo
        .insert(&item(ws_id, "第1集"))
        .await
        .expect("插入笔记失败");
    let mine_b = item_repo
        .insert(&item(ws_id, "第2集"))
        .await
        .expect("插入笔记失败");
    let other_item = item_repo
        .insert(&item(other_ws, "第3集"))
        .await
        .expect("插入笔记失败");

    let questions = vec![
        question(ws_id, mine_a, QuestionType::Single),
        question(ws_id, mine_a, QuestionType::Multi),
        question(ws_id, mine_b, QuestionType::Judge),
        question(other_ws, other_item, QuestionType::Single),
    ];
    let ids = repo.insert_many(&questions).await.expect("批量插入失败");
    assert_eq!(ids.len(), 4);
    assert!(repo.insert_many(&[]).await.expect("空批量").is_empty());

    // 按 id 升序取回；他人空间题目被归属条件过滤。
    let mine = repo.find_by_ids(&ids, user_id).await.expect("查询失败");
    assert_eq!(mine.len(), 3);
    assert_eq!(mine[0].id, ids[0]);
    assert_eq!(mine[1].qtype, QuestionType::Multi);
    assert_eq!(mine[2].answer, Answer::Judge(true));
    assert_eq!(mine[1].options, vec!["甲", "乙", "丙"]);
    let foreign = repo.find_by_ids(&ids, other).await.expect("查询失败");
    assert_eq!(foreign.len(), 1);
    assert_eq!(foreign[0].id, ids[3]);

    // draw：空筛选不过滤，count 截断，id 升序。
    let drawn = repo
        .draw(ws_id, user_id, &[], &[], 2)
        .await
        .expect("抽题失败");
    assert_eq!(drawn.len(), 2);
    assert_eq!(drawn[0].id, ids[0]);
    assert_eq!(drawn[1].id, ids[1]);

    // 按来源与题型过滤。
    let by_source = repo
        .draw(ws_id, user_id, &[mine_a], &[], 10)
        .await
        .expect("抽题失败");
    assert_eq!(by_source.len(), 2);
    let by_type = repo
        .draw(ws_id, user_id, &[], &[QuestionType::Judge], 10)
        .await
        .expect("抽题失败");
    assert_eq!(by_type.len(), 1);
    assert_eq!(by_type[0].qtype, QuestionType::Judge);
    let both = repo
        .draw(ws_id, user_id, &[mine_a], &[QuestionType::Single], 10)
        .await
        .expect("抽题失败");
    assert_eq!(both.len(), 1);
    // 他人空间抽不到：归属 join 是第二道防线（第一道在应用层校验 workspace 归属）。
    assert!(
        repo.draw(other_ws, user_id, &[], &[], 10)
            .await
            .expect("抽题失败")
            .is_empty()
    );
}

#[tokio::test]
async fn quiz_record_and_wrong_item_lifecycle() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "quiz").await;
    let ws_repo = PgWorkspaceRepository::new(pool.clone());
    let ws_id = ws_repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let q_repo = PgQuestionRepository::new(pool.clone());
    let qid = q_repo
        .insert_many(&[question(ws_id, 1, QuestionType::Single)])
        .await
        .expect("插入题目失败")[0];

    let record = PgQuizRecordRepository::new(pool.clone());
    let record_id = record
        .append(&QuizRecord {
            id: 0,
            user_id,
            question_id: qid,
            scope: Some("第 3 集".into()),
            chosen: Some(Chosen::Single(1)),
            is_correct: true,
            created_at: Utc::now(),
        })
        .await
        .expect("记录作答失败");
    assert!(record_id > 0);

    // 首次答错 times=1，重复答错 times=2 且重置掌握标记；未答错过时查不到。
    let wrong = PgWrongItemRepository::new(pool);
    assert!(wrong.find(user_id, qid).await.expect("查询失败").is_none());
    let first = wrong
        .record_mistake(user_id, qid, Utc::now())
        .await
        .expect("记录错题失败");
    assert_eq!(first.times, 1);
    assert!(!first.mastered);
    let second = wrong
        .record_mistake(user_id, qid, Utc::now())
        .await
        .expect("记录错题失败");
    assert_eq!(second.times, 2);
    assert!(!second.mastered);

    let found = wrong
        .find(user_id, qid)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_eq!(found.times, 2);

    let unmastered = wrong.list_unmastered(user_id).await.expect("列表失败");
    assert_eq!(unmastered.len(), 1);
    assert_eq!(unmastered[0].question_id, qid);

    // 标记掌握后不再出现在未掌握列表；重复标记返回 false。
    assert!(
        wrong
            .mark_mastered(user_id, qid, Utc::now())
            .await
            .expect("标记失败")
    );
    assert!(
        !wrong
            .mark_mastered(user_id, qid, Utc::now())
            .await
            .expect("重复标记")
    );
    assert!(
        wrong
            .list_unmastered(user_id)
            .await
            .expect("列表失败")
            .is_empty()
    );
    let mastered = wrong
        .find(user_id, qid)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert!(mastered.mastered);
}

#[tokio::test]
async fn paper_insert_find_and_submit() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "paper").await;
    let other = insert_user(&user_repo, "paper2").await;
    let ws_repo = PgWorkspaceRepository::new(pool.clone());
    let ws_id = ws_repo
        .insert(&new_workspace(user_id))
        .await
        .expect("插入空间失败");
    let repo = PgPaperRepository::new(pool);

    let paper = Paper {
        id: 0,
        user_id,
        workspace_id: ws_id,
        name: Some("模考一".into()),
        config: PaperConfig {
            scope: Some("第 1-3 集".into()),
            question_types: Some(vec![QuestionType::Single]),
            source_item_ids: Some(vec![1, 2]),
            count: 10,
        },
        question_ids: vec![101, 102, 103],
        result: None,
        created_at: Utc::now(),
    };
    let id = repo.insert(&paper).await.expect("插入试卷失败");

    let found = repo
        .find_by_id_and_user(id, user_id)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_eq!(found.name.as_deref(), Some("模考一"));
    assert_eq!(found.config.count, 10);
    assert_eq!(found.question_ids, vec![101, 102, 103]);
    assert!(found.result.is_none());
    // 他人不可见。
    assert!(
        repo.find_by_id_and_user(id, other)
            .await
            .expect("查询失败")
            .is_none()
    );

    // 提交结果后 round-trip。
    let mut submitted = found;
    submitted.result = Some(PaperResult {
        score: 80,
        correct: 8,
        total: 10,
        duration_secs: 1800,
    });
    repo.submit(&submitted).await.expect("提交失败");
    let after = repo
        .find_by_id_and_user(id, user_id)
        .await
        .expect("查询失败")
        .expect("应找到");
    assert_eq!(
        after.result,
        Some(PaperResult {
            score: 80,
            correct: 8,
            total: 10,
            duration_secs: 1800,
        })
    );
}

#[tokio::test]
async fn event_append_and_list_by_user_limit() {
    let Some(pool) = pool().await else {
        return;
    };
    setup(&pool).await;
    let user_repo = PgUserRepository::new(pool.clone());
    let user_id = insert_user(&user_repo, "ev").await;
    let repo = PgEventStore::new(pool);

    let event = |action, payload: Option<&str>| Event {
        id: 0,
        user_id,
        workspace_id: Some(1),
        item_id: Some(2),
        action,
        payload: payload.map(str::to_owned),
        created_at: Utc::now(),
    };
    let e1 = repo
        .append(&event(EventAction::Annotate, Some(r#"{"n":1}"#)))
        .await
        .expect("追加失败");
    let e2 = repo
        .append(&event(EventAction::Answer, Some(r#"{"n":2}"#)))
        .await
        .expect("追加失败");
    repo.append(&event(EventAction::Wrong, None))
        .await
        .expect("追加失败");

    // 取最新 limit 条，按 id 升序返回；payload 以解析后的 Value 比较（jsonb 不保证键序）。
    let listed = repo.list_by_user(user_id, 2).await.expect("查询失败");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, e2);
    assert_eq!(listed[0].action, EventAction::Answer);
    assert_eq!(
        listed[0]
            .payload
            .as_deref()
            .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap()),
        Some(serde_json::json!({"n": 2}))
    );
    assert_eq!(listed[1].action, EventAction::Wrong);
    assert!(listed[1].payload.is_none());
    // 空载荷事件缺省字段为 NULL。
    assert_eq!(listed[1].workspace_id, Some(1));
    assert_eq!(listed[1].item_id, Some(2));

    // 全量按 id 升序。
    let all = repo.list_by_user(user_id, 10).await.expect("查询失败");
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, e1);
    assert_eq!(all[1].id, e2);
}
