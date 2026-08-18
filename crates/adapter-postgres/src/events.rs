//! 事件存储输出端口实现：events 表（追加写，只增不改）。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::EventStore;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use crate::codec::{payload_to_value, value_to_payload};
use crate::map_sqlx_error;

pub struct PgEventStore {
    pool: PgPool,
}

impl PgEventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EventStore for PgEventStore {
    async fn append(&self, event: &Event) -> Result<i64> {
        let row = sqlx::query(
            "insert into events (user_id, workspace_id, item_id, action, payload)
             values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(event.user_id)
        .bind(event.workspace_id)
        .bind(event.item_id)
        .bind(event.action.as_str())
        .bind(payload_to_value(&event.payload)?)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.try_get::<i64, _>("id").map_err(map_sqlx_error)
    }

    async fn list_by_user(&self, user_id: i64, limit: u32) -> Result<Vec<Event>> {
        // 取最新 limit 条后反转成升序（与 inmem 语义一致）。
        let rows = sqlx::query(
            "select id, user_id, workspace_id, item_id, action, payload, created_at
             from events where user_id = $1 order by id desc limit $2",
        )
        .bind(user_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        let mut events: Vec<Event> = rows
            .iter()
            .map(event_from_row)
            .collect::<Result<Vec<_>>>()?;
        events.reverse();
        Ok(events)
    }
}

fn event_from_row(row: &PgRow) -> Result<Event> {
    let action: String = row.try_get("action").map_err(map_sqlx_error)?;
    let payload: Option<serde_json::Value> = row.try_get("payload").map_err(map_sqlx_error)?;
    Ok(Event {
        id: row.try_get::<i64, _>("id").map_err(map_sqlx_error)?,
        user_id: row.try_get::<i64, _>("user_id").map_err(map_sqlx_error)?,
        workspace_id: row
            .try_get::<Option<i64>, _>("workspace_id")
            .map_err(map_sqlx_error)?,
        item_id: row
            .try_get::<Option<i64>, _>("item_id")
            .map_err(map_sqlx_error)?,
        action: action_from_str(&action)?,
        payload: value_to_payload(payload)?,
        created_at: row
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(map_sqlx_error)?,
    })
}

fn action_from_str(s: &str) -> Result<EventAction> {
    match s {
        "annotate" => Ok(EventAction::Annotate),
        "answer" => Ok(EventAction::Answer),
        "wrong" => Ok(EventAction::Wrong),
        "agent_write" => Ok(EventAction::AgentWrite),
        "checkin" => Ok(EventAction::Checkin),
        other => Err(Error::Storage(format!("未知事件动作: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_parses_known_and_rejects_unknown() {
        assert_eq!(action_from_str("annotate").unwrap(), EventAction::Annotate);
        assert_eq!(action_from_str("answer").unwrap(), EventAction::Answer);
        assert_eq!(action_from_str("wrong").unwrap(), EventAction::Wrong);
        assert_eq!(
            action_from_str("agent_write").unwrap(),
            EventAction::AgentWrite
        );
        assert_eq!(action_from_str("checkin").unwrap(), EventAction::Checkin);
        assert!(action_from_str("update").is_err());
    }
}
