//! 训练打卡用例（体育领域包）：CheckinTraining / ListCheckins。
//!
//! 打卡记录以 checkin 事件写入 events 表（只追加），复盘 Agent 经
//! read_events 读取；列表从事件流过滤得到，不落独立表。

use std::sync::Arc;

use chrono::{DateTime, Utc};
use domain::error::{Error, Result};
use domain::event::{Event, EventAction};
use domain::ports::EventStore;
use serde::{Deserialize, Serialize};

/// 打卡入参：练了什么、练了多久、自评几分。
#[derive(Debug, Clone, Deserialize)]
pub struct CheckinInput {
    pub sport: String,
    pub activity: String,
    pub duration_minutes: u32,
    /// 自评 1~5。
    pub rating: u8,
    pub note: Option<String>,
}

/// 打卡记录（写入与列表共用的出参）。
#[derive(Debug, Clone, Serialize)]
pub struct CheckinRecord {
    pub id: i64,
    pub workspace_id: Option<i64>,
    pub sport: String,
    pub activity: String,
    pub duration_minutes: u32,
    pub rating: u8,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub struct TrainingService<E>
where
    E: EventStore + ?Sized,
{
    events: Arc<E>,
}

impl<E> TrainingService<E>
where
    E: EventStore + ?Sized,
{
    pub fn new(events: Arc<E>) -> Self {
        Self { events }
    }

    /// CheckinTraining：校验入参，落一条 checkin 事件。
    pub async fn checkin(
        &self,
        user_id: i64,
        workspace_id: Option<i64>,
        input: CheckinInput,
    ) -> Result<CheckinRecord> {
        if input.sport.trim().is_empty() {
            return Err(Error::Invalid("运动不能为空".to_owned()));
        }
        if input.activity.trim().is_empty() {
            return Err(Error::Invalid("训练内容不能为空".to_owned()));
        }
        if input.duration_minutes == 0 {
            return Err(Error::Invalid("训练时长必须大于 0".to_owned()));
        }
        if !(1..=5).contains(&input.rating) {
            return Err(Error::Invalid("自评分数必须在 1~5 之间".to_owned()));
        }
        let created_at = Utc::now();
        let payload = serde_json::json!({
            "sport": input.sport,
            "activity": input.activity,
            "duration_minutes": input.duration_minutes,
            "rating": input.rating,
            "note": input.note,
        });
        let id = self
            .events
            .append(&Event {
                id: 0,
                user_id,
                workspace_id,
                item_id: None,
                action: EventAction::Checkin,
                payload: Some(payload.to_string()),
                created_at,
            })
            .await?;
        Ok(CheckinRecord {
            id,
            workspace_id,
            sport: input.sport,
            activity: input.activity,
            duration_minutes: input.duration_minutes,
            rating: input.rating,
            note: input.note,
            created_at,
        })
    }

    /// ListCheckins：最近 limit 条打卡记录（事件流过滤，按时间倒序）。
    pub async fn list(&self, user_id: i64, limit: u32) -> Result<Vec<CheckinRecord>> {
        let events = self.events.list_by_user(user_id, limit).await?;
        let mut records: Vec<CheckinRecord> = events
            .iter()
            .filter(|e| e.action == EventAction::Checkin)
            .filter_map(checkin_from_event)
            .collect();
        records.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Ok(records)
    }
}

/// 从 checkin 事件解析打卡记录；payload 无法解析的事件跳过（只读兜底）。
fn checkin_from_event(event: &Event) -> Option<CheckinRecord> {
    let payload: serde_json::Value = serde_json::from_str(event.payload.as_ref()?).ok()?;
    Some(CheckinRecord {
        id: event.id,
        workspace_id: event.workspace_id,
        sport: payload.get("sport")?.as_str()?.to_owned(),
        activity: payload.get("activity")?.as_str()?.to_owned(),
        duration_minutes: payload.get("duration_minutes")?.as_u64()? as u32,
        rating: payload.get("rating")?.as_u64()? as u8,
        note: payload
            .get("note")
            .and_then(|n| n.as_str())
            .map(str::to_owned),
        created_at: event.created_at,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inmem::InMemoryEventStore;
    use domain::event::EventAction;

    fn input() -> CheckinInput {
        CheckinInput {
            sport: "badminton".into(),
            activity: "正手高远球".into(),
            duration_minutes: 60,
            rating: 4,
            note: Some("手感不错".into()),
        }
    }

    async fn svc() -> TrainingService<InMemoryEventStore> {
        TrainingService::new(Arc::new(InMemoryEventStore::default()))
    }

    #[tokio::test]
    async fn checkin_appends_event_with_payload() {
        let s = svc().await;
        let record = s.checkin(1, Some(7), input()).await.unwrap();
        assert_eq!(record.sport, "badminton");
        assert_eq!(record.activity, "正手高远球");
        let events = s.events.list_by_user(1, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(record.id, events[0].id);
        assert_eq!(events[0].action, EventAction::Checkin);
        let payload: serde_json::Value =
            serde_json::from_str(events[0].payload.as_ref().unwrap()).unwrap();
        assert_eq!(payload["duration_minutes"], 60);
        assert_eq!(payload["rating"], 4);
        assert_eq!(payload["note"], "手感不错");
    }

    #[tokio::test]
    async fn list_returns_only_checkins_newest_first() {
        let s = svc().await;
        s.checkin(1, None, input()).await.unwrap();
        // 混入一条非打卡事件（模拟其他行为），不应出现在列表。
        let _ = s
            .events
            .append(&Event {
                id: 0,
                user_id: 1,
                workspace_id: None,
                item_id: None,
                action: EventAction::Answer,
                payload: None,
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        s.checkin(1, None, input()).await.unwrap();
        let list = s.list(1, 10).await.unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].id > list[1].id);
    }

    #[tokio::test]
    async fn checkin_validates_input() {
        let s = svc().await;
        let mut bad = input();
        bad.sport = "  ".into();
        assert!(matches!(
            s.checkin(1, None, bad).await,
            Err(Error::Invalid(_))
        ));
        let mut bad = input();
        bad.activity = "".into();
        assert!(matches!(
            s.checkin(1, None, bad).await,
            Err(Error::Invalid(_))
        ));
        let mut bad = input();
        bad.duration_minutes = 0;
        assert!(matches!(
            s.checkin(1, None, bad).await,
            Err(Error::Invalid(_))
        ));
        let mut bad = input();
        bad.rating = 0;
        assert!(matches!(
            s.checkin(1, None, bad).await,
            Err(Error::Invalid(_))
        ));
        let mut bad = input();
        bad.rating = 6;
        assert!(matches!(
            s.checkin(1, None, bad).await,
            Err(Error::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn list_is_isolated_by_user() {
        let s = svc().await;
        s.checkin(1, None, input()).await.unwrap();
        s.checkin(2, None, input()).await.unwrap();
        assert_eq!(s.list(1, 10).await.unwrap().len(), 1);
        assert_eq!(s.list(2, 10).await.unwrap().len(), 1);
        assert!(s.list(3, 10).await.unwrap().is_empty());
    }
}
