//! 练习与复盘输出端口实现：questions / quiz_records / wrong_items / papers 四张表。
//!
//! options/answer/chosen/config/question_ids/result 为 jsonb 列，此处与领域类型
//! 互转（serde）；读路径在 SQL 层强制 user_id 归属（questions/papers join
//! workspaces），作为隔离第二道防线。

use chrono::{DateTime, Utc};
use domain::error::{Error, Result};
use domain::ports::{
    PaperRepository, QuestionRepository, QuizRecordRepository, WrongItemRepository,
};
use domain::practice::{Paper, Question, QuestionType, QuizRecord, WrongItem, WrongStats};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, QueryBuilder, Row};

use crate::map_sqlx_error;

pub struct PgQuestionRepository {
    pool: PgPool,
}

impl PgQuestionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl QuestionRepository for PgQuestionRepository {
    async fn insert_many(&self, questions: &[Question]) -> Result<Vec<i64>> {
        if questions.is_empty() {
            return Ok(Vec::new());
        }
        let mut qb = QueryBuilder::<sqlx::Postgres>::new(
            "insert into questions (workspace_id, source_item_id, type, stem, options, answer, explanation) ",
        );
        for (i, q) in questions.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push("(");
            qb.push_bind(q.workspace_id);
            qb.push(", ");
            qb.push_bind(q.source_item_id);
            qb.push(", ");
            qb.push_bind(q.qtype.as_str());
            qb.push(", ");
            qb.push_bind(&q.stem);
            qb.push(", ");
            qb.push_bind(
                serde_json::to_value(&q.options)
                    .map_err(|e| Error::Storage(format!("题目选项序列化失败: {e}")))?,
            );
            qb.push(", ");
            qb.push_bind(
                serde_json::to_value(&q.answer)
                    .map_err(|e| Error::Storage(format!("题目答案序列化失败: {e}")))?,
            );
            qb.push(", ");
            qb.push_bind(&q.explanation);
            qb.push(")");
        }
        qb.push(" returning id");
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        rows.iter()
            .map(|row| row.try_get::<i64, _>("id").map_err(map_sqlx_error))
            .collect()
    }

    async fn find_by_ids(&self, ids: &[i64], user_id: i64) -> Result<Vec<Question>> {
        let rows = sqlx::query(
            "select q.id, q.workspace_id, q.source_item_id, q.type, q.stem, q.options,
                    q.answer, q.explanation, q.created_at
             from questions q
             join workspaces w on w.id = q.workspace_id and w.user_id = $2
             where q.id = any($1)
             order by q.id",
        )
        .bind(ids)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(question_from_row).collect()
    }

    async fn draw(
        &self,
        workspace_id: i64,
        source_item_ids: &[i64],
        qtypes: &[QuestionType],
        count: u32,
    ) -> Result<Vec<Question>> {
        // 筛选条件为空时整段省略（与 inmem 语义一致：空数组视为不过滤）。
        let mut qb = QueryBuilder::<sqlx::Postgres>::new(
            "select id, workspace_id, source_item_id, type, stem, options,
                    answer, explanation, created_at
             from questions where workspace_id = ",
        );
        qb.push_bind(workspace_id);
        if !source_item_ids.is_empty() {
            qb.push(" and source_item_id = any(");
            qb.push_bind(source_item_ids);
            qb.push(")");
        }
        if !qtypes.is_empty() {
            qb.push(" and type = any(");
            // 绑定所有权数据（Vec<String>），避免借用随块结束而失效。
            let types: Vec<String> = qtypes.iter().map(|t| t.as_str().to_owned()).collect();
            qb.push_bind(types);
            qb.push(")");
        }
        qb.push(" order by id limit ");
        qb.push_bind(count as i64);
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        rows.iter().map(question_from_row).collect()
    }
}

fn question_from_row(row: &PgRow) -> Result<Question> {
    let qtype: String = row.try_get("type").map_err(map_sqlx_error)?;
    let options: Option<serde_json::Value> = row.try_get("options").map_err(map_sqlx_error)?;
    let answer: serde_json::Value = row.try_get("answer").map_err(map_sqlx_error)?;
    Ok(Question {
        id: row.try_get("id").map_err(map_sqlx_error)?,
        workspace_id: row.try_get("workspace_id").map_err(map_sqlx_error)?,
        source_item_id: row.try_get("source_item_id").map_err(map_sqlx_error)?,
        qtype: qtype_from_str(&qtype)?,
        stem: row.try_get("stem").map_err(map_sqlx_error)?,
        options: serde_json::from_value(options.unwrap_or(serde_json::Value::Array(Vec::new())))
            .map_err(|e| Error::Storage(format!("题目选项解析失败: {e}")))?,
        answer: serde_json::from_value(answer)
            .map_err(|e| Error::Storage(format!("题目答案解析失败: {e}")))?,
        explanation: row.try_get("explanation").map_err(map_sqlx_error)?,
        created_at: row.try_get("created_at").map_err(map_sqlx_error)?,
    })
}

fn qtype_from_str(s: &str) -> Result<QuestionType> {
    match s {
        "single" => Ok(QuestionType::Single),
        "multi" => Ok(QuestionType::Multi),
        "judge" => Ok(QuestionType::Judge),
        other => Err(Error::Storage(format!("未知题型: {other}"))),
    }
}

pub struct PgQuizRecordRepository {
    pool: PgPool,
}

impl PgQuizRecordRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl QuizRecordRepository for PgQuizRecordRepository {
    async fn append(&self, record: &QuizRecord) -> Result<i64> {
        let chosen: Option<serde_json::Value> = record
            .chosen
            .as_ref()
            .map(|c| {
                serde_json::to_value(c).map_err(|e| Error::Storage(format!("作答序列化失败: {e}")))
            })
            .transpose()?;
        let row = sqlx::query(
            "insert into quiz_records (user_id, question_id, scope, chosen, is_correct)
             values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(record.user_id)
        .bind(record.question_id)
        .bind(&record.scope)
        .bind(chosen)
        .bind(record.is_correct)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }
}

pub struct PgWrongItemRepository {
    pool: PgPool,
}

impl PgWrongItemRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl WrongItemRepository for PgWrongItemRepository {
    async fn find(&self, user_id: i64, question_id: i64) -> Result<Option<WrongItem>> {
        sqlx::query(
            "select id, user_id, question_id, times, mastered, updated_at
             from wrong_items where user_id = $1 and question_id = $2",
        )
        .bind(user_id)
        .bind(question_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| wrong_item_from_row(&row))
        .transpose()
    }

    async fn record_mistake(
        &self,
        user_id: i64,
        question_id: i64,
        now: DateTime<Utc>,
    ) -> Result<WrongItem> {
        // upsert：首次答错 times=1；重复答错 times += 1 且重置掌握标记（与 inmem 一致）。
        let row = sqlx::query(
            "insert into wrong_items (user_id, question_id, times, mastered, updated_at)
             values ($1, $2, 1, false, $3)
             on conflict (user_id, question_id) do update
               set times = wrong_items.times + 1, mastered = false,
                   updated_at = excluded.updated_at
             returning id, user_id, question_id, times, mastered, updated_at",
        )
        .bind(user_id)
        .bind(question_id)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        wrong_item_from_row(&row)
    }

    async fn mark_mastered(
        &self,
        user_id: i64,
        question_id: i64,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let result = sqlx::query(
            "update wrong_items set mastered = true, updated_at = $3
             where user_id = $1 and question_id = $2",
        )
        .bind(user_id)
        .bind(question_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_unmastered(&self, user_id: i64) -> Result<Vec<WrongItem>> {
        let rows = sqlx::query(
            "select id, user_id, question_id, times, mastered, updated_at
             from wrong_items where user_id = $1 and not mastered
             order by updated_at",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(wrong_item_from_row).collect()
    }

    async fn stats(&self, user_id: i64, week_ago: DateTime<Utc>) -> Result<WrongStats> {
        let row = sqlx::query(
            "select count(*) as total,
                    count(*) filter (where mastered) as mastered,
                    count(*) filter (where updated_at >= $2) as weekly_new
             from wrong_items where user_id = $1",
        )
        .bind(user_id)
        .bind(week_ago)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(WrongStats {
            total: row.try_get::<i64, _>("total").map_err(map_sqlx_error)? as u32,
            mastered: row.try_get::<i64, _>("mastered").map_err(map_sqlx_error)? as u32,
            weekly_new: row
                .try_get::<i64, _>("weekly_new")
                .map_err(map_sqlx_error)? as u32,
        })
    }
}

fn wrong_item_from_row(row: &PgRow) -> Result<WrongItem> {
    Ok(WrongItem {
        id: row.try_get("id").map_err(map_sqlx_error)?,
        user_id: row.try_get("user_id").map_err(map_sqlx_error)?,
        question_id: row.try_get("question_id").map_err(map_sqlx_error)?,
        // 列类型为 int（i32），域模型用 u32。
        times: row.try_get::<i32, _>("times").map_err(map_sqlx_error)? as u32,
        mastered: row.try_get("mastered").map_err(map_sqlx_error)?,
        updated_at: row.try_get("updated_at").map_err(map_sqlx_error)?,
    })
}

pub struct PgPaperRepository {
    pool: PgPool,
}

impl PgPaperRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PaperRepository for PgPaperRepository {
    async fn insert(&self, paper: &Paper) -> Result<i64> {
        let config = serde_json::to_value(&paper.config)
            .map_err(|e| Error::Storage(format!("试卷配置序列化失败: {e}")))?;
        let question_ids = serde_json::to_value(&paper.question_ids)
            .map_err(|e| Error::Storage(format!("试卷题目快照序列化失败: {e}")))?;
        let row = sqlx::query(
            "insert into papers (user_id, workspace_id, name, config, question_ids)
             values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(paper.user_id)
        .bind(paper.workspace_id)
        .bind(&paper.name)
        .bind(config)
        .bind(question_ids)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn find_by_id_and_user(&self, id: i64, user_id: i64) -> Result<Option<Paper>> {
        sqlx::query(
            "select id, user_id, workspace_id, name, config, question_ids, result, created_at
             from papers where id = $1 and user_id = $2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| paper_from_row(&row))
        .transpose()
    }

    async fn submit(&self, paper: &Paper) -> Result<()> {
        let result: Option<serde_json::Value> = paper
            .result
            .map(|r| {
                serde_json::to_value(r)
                    .map_err(|e| Error::Storage(format!("试卷结果序列化失败: {e}")))
            })
            .transpose()?;
        sqlx::query("update papers set result = $2 where id = $1")
            .bind(paper.id)
            .bind(result)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(())
    }
}

fn paper_from_row(row: &PgRow) -> Result<Paper> {
    let config: serde_json::Value = row.try_get("config").map_err(map_sqlx_error)?;
    let question_ids: serde_json::Value = row.try_get("question_ids").map_err(map_sqlx_error)?;
    let result: Option<serde_json::Value> = row.try_get("result").map_err(map_sqlx_error)?;
    Ok(Paper {
        id: row.try_get("id").map_err(map_sqlx_error)?,
        user_id: row.try_get("user_id").map_err(map_sqlx_error)?,
        workspace_id: row.try_get("workspace_id").map_err(map_sqlx_error)?,
        name: row.try_get("name").map_err(map_sqlx_error)?,
        config: serde_json::from_value(config)
            .map_err(|e| Error::Storage(format!("试卷配置解析失败: {e}")))?,
        question_ids: serde_json::from_value(question_ids)
            .map_err(|e| Error::Storage(format!("试卷题目快照解析失败: {e}")))?,
        result: result
            .map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| Error::Storage(format!("试卷结果解析失败: {e}")))
            })
            .transpose()?,
        created_at: row.try_get("created_at").map_err(map_sqlx_error)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::practice::{Answer, Chosen, PaperConfig};
    use std::collections::BTreeSet;

    #[test]
    fn qtype_parses_known_and_rejects_unknown() {
        assert_eq!(qtype_from_str("single").unwrap(), QuestionType::Single);
        assert_eq!(qtype_from_str("multi").unwrap(), QuestionType::Multi);
        assert_eq!(qtype_from_str("judge").unwrap(), QuestionType::Judge);
        assert!(qtype_from_str("essay").is_err());
    }

    #[test]
    fn domain_values_round_trip_through_json() {
        let answer = Answer::Multi(BTreeSet::from([0, 3]));
        let v = serde_json::to_value(&answer).unwrap();
        assert_eq!(serde_json::from_value::<Answer>(v).unwrap(), answer);

        let chosen = Chosen::Judge(false);
        let v = serde_json::to_value(&chosen).unwrap();
        assert_eq!(serde_json::from_value::<Chosen>(v).unwrap(), chosen);

        let config = PaperConfig {
            scope: Some("第 3 集".into()),
            question_types: Some(vec![QuestionType::Single]),
            source_item_ids: Some(vec![1, 2]),
            count: 5,
        };
        let v = serde_json::to_value(&config).unwrap();
        assert_eq!(serde_json::from_value::<PaperConfig>(v).unwrap(), config);
    }
}
