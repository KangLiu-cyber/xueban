//! 用户自定义 Skill 输出端口实现：skills 表。
//!
//! 查询/删除在 SQL 层强制 user_id 归属条件（隔离第二道防线）；同用户重名
//! 由唯一约束拒绝（map_sqlx_error → Conflict）。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::error::Result;
use domain::ports::SkillRepository;
use domain::skill::UserSkill;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::map_sqlx_error;

pub struct PgSkillRepository {
    pool: PgPool,
}

impl PgSkillRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SkillRepository for PgSkillRepository {
    async fn insert(&self, skill: &UserSkill) -> Result<i64> {
        let row = sqlx::query(
            "insert into skills (user_id, name, description, script)
             values ($1, $2, $3, $4) returning id",
        )
        .bind(skill.user_id)
        .bind(&skill.name)
        .bind(&skill.description)
        .bind(&skill.script)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn list_by_user(&self, user_id: i64) -> Result<Vec<UserSkill>> {
        let rows = sqlx::query(
            "select id, user_id, name, description, script, created_at
             from skills where user_id = $1 order by id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.iter().map(skill_from_row).collect()
    }

    async fn find_by_name_and_user(&self, name: &str, user_id: i64) -> Result<Option<UserSkill>> {
        sqlx::query(
            "select id, user_id, name, description, script, created_at
             from skills where name = $1 and user_id = $2",
        )
        .bind(name)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| skill_from_row(&row))
        .transpose()
    }

    async fn delete(&self, id: i64, user_id: i64) -> Result<bool> {
        let result = sqlx::query("delete from skills where id = $1 and user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }
}

fn skill_from_row(row: &PgRow) -> Result<UserSkill> {
    Ok(UserSkill {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        user_id: row.try_get::<i64, _>("user_id").map_err(map_sqlx_error)?,
        name: row.try_get::<String, _>("name").map_err(map_sqlx_error)?,
        description: row
            .try_get::<String, _>("description")
            .map_err(map_sqlx_error)?,
        script: row
            .try_get::<Option<String>, _>("script")
            .map_err(map_sqlx_error)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
    })
}
