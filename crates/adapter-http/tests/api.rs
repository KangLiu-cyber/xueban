//! REST API 集成测试（env-gated）：注册/登录 → 空间 → 批注 → 刷题 →
//! 错题 → 组卷交卷 全链路，另含鉴权与参数校验。
//!
//! 未设置 DATABASE_URL 时全部跳过（CI 无数据库）；题目/笔记由
//! Agent 经 MCP 写入，测试用仓储直接播种（仓库即被测试边界）。

mod common;

use adapter_http::{AppState, router};
use adapter_postgres::{
    Argon2PasswordHasher, FsAttachmentStorage, PgAnnotationRepository, PgAttachmentRepository,
    PgEventStore, PgItemRepository, PgPaperRepository, PgQuestionRepository,
    PgQuizRecordRepository, PgSkillRepository, PgTokenRepository, PgUserRepository,
    PgWorkspaceRepository, PgWrongItemRepository, RandomCredentialIssuer,
};
use application::agent::AgentService;
use application::attachments::AttachmentService;
use application::auth::AuthService;
use application::paper::PaperService;
use application::quiz::QuizService;
use application::space::SpaceService;
use application::training::TrainingService;
use application::wrong::WrongService;
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use chrono::Utc;
use domain::event::EventAction;
use domain::ports::{
    AnnotationRepository, AttachmentRepository, AttachmentStorage, CredentialIssuer, EventStore,
    ItemRepository, PaperRepository, PasswordHasher, QuestionRepository, QuizRecordRepository,
    SkillRepository, TokenRepository, UserRepository, WorkspaceRepository, WrongItemRepository,
};
use domain::practice::{Answer, Question, QuestionType};
use domain::space::{Creator, Item, ItemKind};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

/// 与 bootstrap 相同的六边形组装（P1-10）：仓储 → 用例服务 → 注入 REST 适配器。
/// 额外组装附件服务（真实磁盘目录），返回临时附件目录供测试清理与落盘断言。
async fn app_with_dir() -> Option<(Router, PathBuf)> {
    let pool = common::pool().await?;
    common::setup(&pool).await;
    let users: Arc<dyn UserRepository + Send + Sync> =
        Arc::new(PgUserRepository::new(pool.clone()));
    let tokens: Arc<dyn TokenRepository + Send + Sync> =
        Arc::new(PgTokenRepository::new(pool.clone()));
    let hasher: Arc<dyn PasswordHasher + Send + Sync> = Arc::new(Argon2PasswordHasher);
    let issuer: Arc<dyn CredentialIssuer + Send + Sync> = Arc::new(RandomCredentialIssuer);
    let workspaces: Arc<dyn WorkspaceRepository + Send + Sync> =
        Arc::new(PgWorkspaceRepository::new(pool.clone()));
    let items: Arc<dyn ItemRepository + Send + Sync> =
        Arc::new(PgItemRepository::new(pool.clone()));
    let annotations: Arc<dyn AnnotationRepository + Send + Sync> =
        Arc::new(PgAnnotationRepository::new(pool.clone()));
    let questions: Arc<dyn QuestionRepository + Send + Sync> =
        Arc::new(PgQuestionRepository::new(pool.clone()));
    let quiz_records: Arc<dyn QuizRecordRepository + Send + Sync> =
        Arc::new(PgQuizRecordRepository::new(pool.clone()));
    let wrong_items: Arc<dyn WrongItemRepository + Send + Sync> =
        Arc::new(PgWrongItemRepository::new(pool.clone()));
    let papers: Arc<dyn PaperRepository + Send + Sync> =
        Arc::new(PgPaperRepository::new(pool.clone()));
    let skills_repo: Arc<dyn SkillRepository + Send + Sync> =
        Arc::new(PgSkillRepository::new(pool.clone()));
    let attachment_repo: Arc<dyn AttachmentRepository + Send + Sync> =
        Arc::new(PgAttachmentRepository::new(pool.clone()));
    let dir = std::env::temp_dir().join(format!("xueban-att-{}", common::stamp()));
    std::fs::create_dir_all(&dir).expect("创建临时附件目录失败");
    let storage: Arc<dyn AttachmentStorage + Send + Sync> =
        Arc::new(FsAttachmentStorage::new(dir.clone()));
    let events: Arc<dyn EventStore + Send + Sync> = Arc::new(PgEventStore::new(pool));

    let auth = Arc::new(AuthService::new(users, tokens, hasher, issuer));
    let space = Arc::new(SpaceService::new(
        workspaces.clone(),
        items.clone(),
        annotations,
        events.clone(),
    ));
    let quiz = Arc::new(QuizService::new(
        workspaces.clone(),
        items.clone(),
        questions.clone(),
        quiz_records,
        wrong_items.clone(),
        events.clone(),
    ));
    let wrong = Arc::new(WrongService::new(wrong_items.clone(), questions.clone()));
    let paper = Arc::new(PaperService::new(
        workspaces.clone(),
        items.clone(),
        questions.clone(),
        papers,
        wrong_items,
        events.clone(),
    ));
    let agent = Arc::new(AgentService::new(
        workspaces,
        items.clone(),
        questions,
        events.clone(),
        skills_repo,
        Vec::new(),
    ));
    let training = Arc::new(TrainingService::new(events));
    let attachments = Arc::new(AttachmentService::new(items, attachment_repo, storage));
    Some((
        router(AppState::new(
            auth,
            space,
            quiz,
            wrong,
            paper,
            agent,
            attachments,
            training,
            "https://mcp.example.com/mcp".into(),
        )),
        dir,
    ))
}

async fn app() -> Option<Router> {
    app_with_dir().await.map(|(r, _)| r)
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .expect("构建请求失败"),
        None => builder.body(Body::empty()).expect("构建请求失败"),
    };
    let resp = app.clone().oneshot(req).await.expect("请求失败");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("读响应失败")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn register(app: &Router, tag: &str) -> (String, Value) {
    let (status, body) = send(
        app,
        Method::POST,
        "/api/v1/auth/register",
        None,
        Some(json!({
            "account": format!("{tag}_{}", common::stamp()),
            "password": "password1",
            "nickname": format!("{tag}-昵称"),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "注册失败: {body}");
    let token = body["token"].as_str().expect("注册响应无 token").to_owned();
    (token, body)
}

async fn create_workspace(app: &Router, token: &str, name: &str) -> Value {
    let (status, body) = send(
        app,
        Method::POST,
        "/api/v1/workspaces",
        Some(token),
        Some(json!({
            "name": name,
            "exam_goal": "目标",
            "exam_date": null,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "建空间失败: {body}");
    body
}

/// 原始字节请求（附件上传/读取）：返回状态 + 完整响应头 + 响应字节。
async fn send_raw(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let req = builder.body(Body::from(body)).expect("构建请求失败");
    let resp = app.clone().oneshot(req).await.expect("请求失败");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("读响应失败")
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}

/// 合法 PNG 字节（魔数嗅探通过的最小样本）。
fn png_bytes() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01, 0x02, 0x03,
    ]
}

/// 播种一个笔记集（Agent 侧产物的替身）与 n 道单选题，返回 (item_id, 题号列表)。
async fn seed_questions(pool: &sqlx::PgPool, ws_id: i64, n: u32) -> (i64, Vec<i64>) {
    let item_repo = PgItemRepository::new(pool.clone());
    let item = Item {
        id: 0,
        workspace_id: ws_id,
        parent_id: None,
        kind: ItemKind::Note,
        name: format!("第1集_{}", common::stamp()),
        content: Some("内容".into()),
        created_by: Creator::Agent,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let item_id = item_repo.insert(&item).await.expect("插入笔记失败");
    let q_repo = PgQuestionRepository::new(pool.clone());
    let questions: Vec<Question> = (0..n)
        .map(|i| Question {
            id: 0,
            workspace_id: ws_id,
            source_item_id: item_id,
            qtype: QuestionType::Single,
            stem: format!("1+{}=", i + 1),
            options: vec!["1".into(), "2".into()],
            answer: Answer::Single(1),
            explanation: None,
            created_at: Utc::now(),
        })
        .collect();
    let ids = q_repo.insert_many(&questions).await.expect("插入题目失败");
    (item_id, ids)
}

#[tokio::test]
async fn register_login_logout_flow() {
    let Some(app) = app().await else {
        return;
    };
    let (token, body) = register(&app, "alice").await;
    // 用户响应不含 password_hash。
    assert!(body["user"].get("password_hash").is_none());
    assert_eq!(body["user"]["nickname"], json!("alice-昵称"));

    // 登录换发新 token，旧 token 立即失效。
    let (status, login) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "account": body["user"]["account"],
            "password": "password1",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "登录失败: {login}");
    let new_token = login["token"].as_str().expect("登录响应无 token");
    assert_ne!(new_token, token);
    let (status, _) = send(&app, Method::GET, "/api/v1/workspaces", Some(&token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "旧 token 应已失效");

    // 注销幂等（吊销后的 token 也能注销）。
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        Some(new_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        Some(new_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "重复注销应幂等");
}

#[tokio::test]
async fn duplicate_account_and_weak_password_rejected() {
    let Some(app) = app().await else {
        return;
    };
    let (_, body) = register(&app, "dup").await;
    let account = body["user"]["account"].clone();
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/register",
        None,
        Some(json!({ "account": account, "password": "password1" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/register",
        None,
        Some(json!({ "account": "weak", "password": "short" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn protected_routes_require_token() {
    let Some(app) = app().await else {
        return;
    };
    let (status, _) = send(&app, Method::GET, "/api/v1/workspaces", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/workspaces",
        Some("usr_garbage"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn workspace_crud_and_user_isolation() {
    let Some(app) = app().await else {
        return;
    };
    let (token, _) = register(&app, "u1").await;
    let ws = create_workspace(&app, &token, "备考").await;
    let ws_id = ws["id"].as_i64().expect("无空间 id");

    let (status, list) = send(&app, Method::GET, "/api/v1/workspaces", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("应为数组").len(), 1);

    // 他人看不到、改不到。
    let (other_token, _) = register(&app, "u2").await;
    let (status, list2) = send(
        &app,
        Method::GET,
        "/api/v1/workspaces",
        Some(&other_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list2.as_array().expect("应为数组").is_empty());
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/workspaces/{ws_id}/tree"),
        Some(&other_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "他人空间应 404");

    // 更新目标与日期。
    let (status, updated) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/workspaces/{ws_id}"),
        Some(&token),
        Some(json!({
            "name": "冲刺",
            "exam_goal": "新目标",
            "exam_date": "2026-12-01",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "更新失败: {updated}");
    assert_eq!(updated["name"], json!("冲刺"));
    assert_eq!(updated["exam_date"], json!("2026-12-01"));
}

#[tokio::test]
async fn annotation_lifecycle() {
    let Some(app) = app().await else {
        return;
    };
    let pool = common::pool().await.expect("连接池丢失");
    let (token, _) = register(&app, "ann").await;
    let ws = create_workspace(&app, &token, "备考").await;
    let ws_id = ws["id"].as_i64().expect("无空间 id");
    let (item_id, _) = seed_questions(&pool, ws_id, 1).await;

    let (status, ann) = send(
        &app,
        Method::POST,
        &format!("/api/v1/items/{item_id}/annotations"),
        Some(&token),
        Some(json!({ "anchor": "L3-L5", "text": "易错点" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "加批注失败: {ann}");
    assert_eq!(ann["author"], json!("user"));
    let ann_id = ann["id"].as_i64().expect("无批注 id");

    // 笔记详情携带批注。
    let (status, bundle) = send(
        &app,
        Method::GET,
        &format!("/api/v1/items/{item_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bundle["item"]["id"], json!(item_id));
    assert_eq!(bundle["annotations"].as_array().expect("应为数组").len(), 1);

    // 编辑自己的批注文本，GET 反映新文本。
    let (status, edited) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/annotations/{ann_id}"),
        Some(&token),
        Some(json!({ "text": "已修订的易错点" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "编辑批注失败: {edited}");
    assert_eq!(edited["id"], json!(ann_id));
    assert_eq!(edited["text"], json!("已修订的易错点"));
    let (status, bundle) = send(
        &app,
        Method::GET,
        &format!("/api/v1/items/{item_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let annotations = bundle["annotations"].as_array().expect("应为数组");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["text"], json!("已修订的易错点"));

    // 他人不能编辑（归属校验）。
    let (other_token, _) = register(&app, "ann2").await;
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/annotations/{ann_id}"),
        Some(&other_token),
        Some(json!({ "text": "越权" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "他人编辑应 404");

    // 删除 + 幂等性边界：重复删除 404。
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/annotations/{ann_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/annotations/{ann_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn quiz_wrong_and_paper_full_loop() {
    let Some(app) = app().await else {
        return;
    };
    let pool = common::pool().await.expect("连接池丢失");
    let (token, _) = register(&app, "loop").await;
    let ws = create_workspace(&app, &token, "备考").await;
    let ws_id = ws["id"].as_i64().expect("无空间 id");
    let (item_id, q_ids) = seed_questions(&pool, ws_id, 3).await;

    // 抽题：带集范围 + 不带范围各一次。
    let (status, drawn) = send(
        &app,
        Method::GET,
        &format!("/api/v1/quiz/questions?workspace_id={ws_id}&scope={item_id}&count=3"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "抽题失败: {drawn}");
    let drawn = drawn.as_array().expect("应为数组");
    assert_eq!(drawn.len(), 3);
    assert!(drawn[0].get("answer").is_none(), "简述不得含答案");
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/quiz/questions?workspace_id={ws_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 答对 / 答错。
    let (status, ok) = send(
        &app,
        Method::POST,
        "/api/v1/quiz/answer",
        Some(&token),
        Some(json!({
            "question_id": q_ids[0],
            "chosen": 1,
            "scope": "第1集",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ok["is_correct"], json!(true));
    let (status, bad) = send(
        &app,
        Method::POST,
        "/api/v1/quiz/answer",
        Some(&token),
        Some(json!({ "question_id": q_ids[1], "chosen": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bad["is_correct"], json!(false));

    // 错题本：1 条且带题干；标记掌握后清空。
    let (status, wrongs) = send(&app, Method::GET, "/api/v1/wrong", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let wrongs = wrongs.as_array().expect("应为数组");
    assert_eq!(wrongs.len(), 1);
    assert_eq!(wrongs[0]["question"]["stem"], json!("1+2="));
    let wrong_qid = wrongs[0]["wrong"]["question_id"].as_i64().expect("无题号");
    assert_eq!(wrong_qid, q_ids[1]);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/wrong/{wrong_qid}/master"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, wrongs) = send(&app, Method::GET, "/api/v1/wrong", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(wrongs.as_array().expect("应为数组").is_empty());

    // 组卷：快照冻结 → 读卷一致 → 交卷判分 → 重复交卷 409。
    let (status, paper) = send(
        &app,
        Method::POST,
        "/api/v1/papers",
        Some(&token),
        Some(json!({
            "workspace_id": ws_id,
            "name": "模考一",
            "config": { "count": 3, "source_item_ids": [item_id] },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "组卷失败: {paper}");
    let paper_id = paper["paper"]["id"].as_i64().expect("无试卷 id");
    assert_eq!(paper["questions"].as_array().expect("应为数组").len(), 3);
    let (status, again) = send(
        &app,
        Method::GET,
        &format!("/api/v1/papers/{paper_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        again["paper"]["question_ids"],
        paper["paper"]["question_ids"]
    );

    let (status, result) = send(
        &app,
        Method::POST,
        &format!("/api/v1/papers/{paper_id}/submit"),
        Some(&token),
        Some(json!({
            "answers": [
                { "question_id": q_ids[0], "chosen": 1 },
                { "question_id": q_ids[1], "chosen": 0 },
            ],
            "duration_secs": 300,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "交卷失败: {result}");
    assert_eq!(result["correct"], json!(1)); // q0 答对、q1 答错、q2 缺答计错
    assert_eq!(result["total"], json!(3));
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/papers/{paper_id}/submit"),
        Some(&token),
        Some(json!({ "answers": [], "duration_secs": 10 })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "重复交卷应 409");

    // 组卷参数校验：count 越界 400。
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/papers",
        Some(&token),
        Some(json!({
            "workspace_id": ws_id,
            "config": { "count": 0 },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn malformed_json_maps_to_400() {
    let Some(app) = app().await else {
        return;
    };
    let (token, _) = register(&app, "badjson").await;
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/workspaces",
        Some(&token),
        Some(json!({ "name": "缺字段" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn agent_credential_rotate_flow() {
    let Some(app) = app().await else {
        return;
    };
    let (token, _) = register(&app, "agent").await;
    // 未换发时读不到。
    let (status, _) = send(
        &app,
        Method::GET,
        "/api/v1/agent/credential",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, first) = send(
        &app,
        Method::POST,
        "/api/v1/agent/credential/rotate",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "换发失败: {first}");
    assert!(first["token"].as_str().expect("无凭证").starts_with("usr_"));
    assert_eq!(first["endpoint"], json!("https://mcp.example.com/mcp"));

    // 读取与换发一致；再换发后旧凭证失效。
    let (status, read) = send(
        &app,
        Method::GET,
        "/api/v1/agent/credential",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["token"], first["token"]);
    let (status, second) = send(
        &app,
        Method::POST,
        "/api/v1/agent/credential/rotate",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(second["token"], first["token"]);
}

// ---- 附件（§8.1）：上传 / 读取 / 删除 / 隔离 / 级联 ----

/// 播种一个 note（或 dir）item，返回 item_id（Agent 侧产物的替身）。
async fn seed_item(
    pool: &sqlx::PgPool,
    ws_id: i64,
    parent_id: Option<i64>,
    kind: ItemKind,
    name: &str,
) -> i64 {
    let item_repo = PgItemRepository::new(pool.clone());
    let item = Item {
        id: 0,
        workspace_id: ws_id,
        parent_id,
        kind,
        name: format!("{name}_{}", common::stamp()),
        content: Some("内容".into()),
        created_by: Creator::Agent,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    item_repo.insert(&item).await.expect("插入 item 失败")
}

/// 目录内已上传文件数（递归）。
fn disk_files(dir: &std::path::Path) -> usize {
    fn walk(p: &std::path::Path) -> usize {
        std::fs::read_dir(p)
            .map(|rd| {
                rd.flatten()
                    .map(|e| {
                        if e.path().is_dir() {
                            walk(&e.path())
                        } else {
                            1
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    walk(dir)
}

#[tokio::test]
async fn attachment_lifecycle_and_isolation() {
    let Some((app, dir)) = app_with_dir().await else {
        return;
    };
    let (alice, alice_body) = register(&app, "att_a").await;
    let alice_id = alice_body["user"]["id"].as_i64().expect("无用户 id");
    let (bob, _) = register(&app, "att_b").await;
    let ws = create_workspace(&app, &alice, "集").await;
    let ws_id = ws["id"].as_i64().expect("无空间 id");

    let pool = common::pool().await.expect("池已存在");
    let note_id = seed_item(&pool, ws_id, None, ItemKind::Note, "笔记").await;
    let bytes = png_bytes();

    // 上传：201 + 元数据（魔数权威 mime）+ 磁盘落盘 {user_id}/{uuid}。
    let (status, headers, body) = send_raw(
        &app,
        Method::POST,
        &format!("/api/v1/items/{note_id}/attachments?name=图.png"),
        Some(&alice),
        Some("image/png"),
        bytes.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "上传失败: {body:?}");
    let att: Value = serde_json::from_slice(&body).expect("响应非 JSON");
    let att_id = att["id"].as_i64().expect("无附件 id");
    assert_eq!(att["mime"], json!("image/png"));
    assert_eq!(att["filename"], json!("图.png"));
    assert_eq!(att["size_bytes"], json!(bytes.len() as i64));
    assert_eq!(
        headers.get("content-type").map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
    let uuid = att["uuid"].as_str().expect("无 uuid");
    let disk_path = dir.join(alice_id.to_string()).join(uuid);
    assert!(disk_path.is_file(), "文件未落盘: {disk_path:?}");
    assert_eq!(std::fs::read(&disk_path).expect("读盘失败"), bytes);

    // 读取：200 + 存储 mime + nosniff + 原字节。
    let (status, headers, body) = send_raw(
        &app,
        Method::GET,
        &format!("/api/v1/attachments/{att_id}"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get("content-type").map(|v| v.to_str().unwrap()),
        Some("image/png")
    );
    assert_eq!(
        headers
            .get("x-content-type-options")
            .map(|v| v.to_str().unwrap()),
        Some("nosniff")
    );
    assert_eq!(body, bytes);

    // 无 token 401；跨用户 404（读取与删除双隔离）。
    let (status, _, _) = send_raw(
        &app,
        Method::GET,
        &format!("/api/v1/attachments/{att_id}"),
        None,
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = send_raw(
        &app,
        Method::GET,
        &format!("/api/v1/attachments/{att_id}"),
        Some(&bob),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/attachments/{att_id}"),
        Some(&bob),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 单删：204 → 读取 404 → 磁盘文件消失。
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/attachments/{att_id}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send_raw(
        &app,
        Method::GET,
        &format!("/api/v1/attachments/{att_id}"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!disk_path.exists(), "删除后文件应消失");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn attachment_validation_and_limits() {
    let Some((app, dir)) = app_with_dir().await else {
        return;
    };
    let (alice, _) = register(&app, "att_v").await;
    let ws = create_workspace(&app, &alice, "集").await;
    let ws_id = ws["id"].as_i64().expect("无空间 id");

    let pool = common::pool().await.expect("池已存在");
    let note_id = seed_item(&pool, ws_id, None, ItemKind::Note, "笔记").await;
    let dir_id = seed_item(&pool, ws_id, None, ItemKind::Dir, "目录").await;

    // svg 白名单拒绝（XSS 面）。
    let (status, _, body) = send_raw(
        &app,
        Method::POST,
        &format!("/api/v1/items/{note_id}/attachments"),
        Some(&alice),
        Some("image/svg+xml"),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "svg 应被拒: {body:?}");

    // 声明 png 但字节不是图片：魔数嗅探权威拒绝。
    let (status, _, _) = send_raw(
        &app,
        Method::POST,
        &format!("/api/v1/items/{note_id}/attachments"),
        Some(&alice),
        Some("image/png"),
        b"not an image at all".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 超 10MB 业务上限 → 400。
    let mut huge = png_bytes();
    huge.extend(std::iter::repeat_n(0u8, 10 * 1024 * 1024));
    let (status, _, _) = send_raw(
        &app,
        Method::POST,
        &format!("/api/v1/items/{note_id}/attachments"),
        Some(&alice),
        Some("image/png"),
        huge,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 13MB > 12MB 路由缓冲上限 → 413（RequestBodyLimitLayer 只作用上传子路由）。
    let mut over = png_bytes();
    over.extend(std::iter::repeat_n(0u8, 13 * 1024 * 1024));
    let (status, _, _) = send_raw(
        &app,
        Method::POST,
        &format!("/api/v1/items/{note_id}/attachments"),
        Some(&alice),
        Some("image/png"),
        over,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // 目录不能挂附件（is_note 校验）。
    let (status, _, body) = send_raw(
        &app,
        Method::POST,
        &format!("/api/v1/items/{dir_id}/attachments"),
        Some(&alice),
        Some("image/png"),
        png_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "目录挂附件应被拒: {body:?}"
    );

    // 以上全部失败路径不落盘。
    assert_eq!(disk_files(&dir), 0, "失败路径不应写盘");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn attachment_cascade_delete_with_item_tree() {
    let Some((app, dir)) = app_with_dir().await else {
        return;
    };
    let (alice, _) = register(&app, "att_c").await;
    let ws = create_workspace(&app, &alice, "集").await;
    let ws_id = ws["id"].as_i64().expect("无空间 id");

    let pool = common::pool().await.expect("池已存在");
    // 父 note 带子 note（两层深度），各自挂一个附件。
    let parent = seed_item(&pool, ws_id, None, ItemKind::Note, "父笔记").await;
    let child = seed_item(&pool, ws_id, Some(parent), ItemKind::Note, "子笔记").await;
    let a1 = {
        let (status, _, body) = send_raw(
            &app,
            Method::POST,
            &format!("/api/v1/items/{parent}/attachments"),
            Some(&alice),
            Some("image/png"),
            png_bytes(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice::<Value>(&body).expect("非 JSON")["id"]
            .as_i64()
            .expect("无附件 id")
    };
    let a2 = {
        let (status, _, body) = send_raw(
            &app,
            Method::POST,
            &format!("/api/v1/items/{child}/attachments"),
            Some(&alice),
            Some("image/gif"),
            b"GIF89a\x01\x00\x01\x00".to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice::<Value>(&body).expect("非 JSON")["id"]
            .as_i64()
            .expect("无附件 id")
    };
    assert_eq!(disk_files(&dir), 2, "两个附件都应落盘");

    // 删父 item：子树附件先清文件，再删 item（DB 级联清行）。
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/items/{parent}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "删父 item 失败");
    let (status, _, _) = send_raw(
        &app,
        Method::GET,
        &format!("/api/v1/attachments/{a1}"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send_raw(
        &app,
        Method::GET,
        &format!("/api/v1/attachments/{a2}"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(disk_files(&dir), 0, "级联删除后磁盘应清空");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn training_checkin_and_list_flow() {
    let Some(app) = app().await else {
        return;
    };
    let pool = common::pool().await.expect("连接池丢失");
    let (token, body) = register(&app, "sport").await;
    let user_id = body["user"]["id"].as_i64().expect("无用户 id");

    // 正常打卡（带备注）→ 记录字段齐全。
    let (status, rec) = send(
        &app,
        Method::POST,
        "/api/v1/training/checkin",
        Some(&token),
        Some(json!({
            "sport": "badminton",
            "activity": "正手高远球",
            "duration_minutes": 60,
            "rating": 4,
            "note": "手感不错",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "打卡失败: {rec}");
    assert_eq!(rec["sport"], json!("badminton"));
    assert_eq!(rec["activity"], json!("正手高远球"));
    assert_eq!(rec["duration_minutes"], json!(60));
    assert_eq!(rec["rating"], json!(4));
    assert_eq!(rec["note"], json!("手感不错"));

    // 无备注打卡。
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/training/checkin",
        Some(&token),
        Some(json!({
            "sport": "core",
            "activity": "平板支撑",
            "duration_minutes": 30,
            "rating": 5,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 列表：新→旧；limit 生效。
    let (status, list) = send(
        &app,
        Method::GET,
        "/api/v1/training/checkins?limit=10",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().expect("应为数组");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["sport"], json!("core"), "应按时间倒序");
    assert_eq!(list[1]["activity"], json!("正手高远球"));
    let (status, one) = send(
        &app,
        Method::GET,
        "/api/v1/training/checkins?limit=1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one.as_array().expect("应为数组").len(), 1);

    // 参数校验 400：非法 rating / 空运动 / 空内容 / 零时长 / limit 越界。
    for bad in [
        json!({"sport": "badminton", "activity": "x", "duration_minutes": 1, "rating": 0}),
        json!({"sport": "badminton", "activity": "x", "duration_minutes": 1, "rating": 6}),
        json!({"sport": "  ", "activity": "x", "duration_minutes": 1, "rating": 3}),
        json!({"sport": "badminton", "activity": " ", "duration_minutes": 1, "rating": 3}),
        json!({"sport": "badminton", "activity": "x", "duration_minutes": 0, "rating": 3}),
    ] {
        let (status, body) = send(
            &app,
            Method::POST,
            "/api/v1/training/checkin",
            Some(&token),
            Some(bad),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "应 400: {body}");
    }
    for limit in [0, 101] {
        let (status, _) = send(
            &app,
            Method::GET,
            &format!("/api/v1/training/checkins?limit={limit}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "limit={limit} 应 400");
    }

    // 用户隔离：他人看不到任何打卡。
    let (other_token, _) = register(&app, "sport2").await;
    let (status, list2) = send(
        &app,
        Method::GET,
        "/api/v1/training/checkins?limit=10",
        Some(&other_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list2.as_array().expect("应为数组").is_empty());

    // 未鉴权 401。
    let (status, _) = send(
        &app,
        Method::GET,
        "/api/v1/training/checkins?limit=10",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/training/checkin",
        None,
        Some(json!({"sport": "badminton", "activity": "x", "duration_minutes": 1, "rating": 3})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 事件流落库：复盘 Agent 走 read_events 应读到两条 checkin 事件。
    let events = PgEventStore::new(pool.clone())
        .list_by_user(user_id, 10)
        .await
        .expect("读事件失败");
    let checkins: Vec<_> = events
        .iter()
        .filter(|e| e.action == EventAction::Checkin)
        .collect();
    assert_eq!(checkins.len(), 2, "应落 2 条 checkin 事件");
    // 事件流为升序（先 badminton 后 core），与 HTTP 列表的倒序相反。
    let payload: Value = serde_json::from_str(checkins[0].payload.as_ref().expect("无 payload"))
        .expect("坏 payload");
    assert_eq!(payload["sport"], json!("badminton"));
    assert_eq!(payload["duration_minutes"], json!(60));
    assert_eq!(payload["rating"], json!(4));
    let payload: Value = serde_json::from_str(checkins[1].payload.as_ref().expect("无 payload"))
        .expect("坏 payload");
    assert_eq!(payload["sport"], json!("core"));
    assert!(
        payload["note"].is_null(),
        "无备注时 payload 的 note 应为 null"
    );
}
