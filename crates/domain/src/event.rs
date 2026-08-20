//! 协作事件上下文：Event 只追加不修改，按用户回放。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 行为类型：annotate/answer/wrong/agent_write/checkin/video_submit 等。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventAction {
    Annotate,
    Answer,
    Wrong,
    AgentWrite,
    Checkin,
    /// 视频题作答：用户上传训练视频 + 训练想法，待 AI 复盘。
    VideoSubmit,
}

impl EventAction {
    pub fn as_str(self) -> &'static str {
        match self {
            EventAction::Annotate => "annotate",
            EventAction::Answer => "answer",
            EventAction::Wrong => "wrong",
            EventAction::AgentWrite => "agent_write",
            EventAction::Checkin => "checkin",
            EventAction::VideoSubmit => "video_submit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub user_id: i64,
    pub workspace_id: Option<i64>,
    pub item_id: Option<i64>,
    pub action: EventAction,
    /// 行为快照，JSON 文本（领域层不依赖 JSON 库；应用层负责序列化）。
    pub payload: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_str_matches_docs() {
        assert_eq!(EventAction::Annotate.as_str(), "annotate");
        assert_eq!(EventAction::Answer.as_str(), "answer");
        assert_eq!(EventAction::Wrong.as_str(), "wrong");
        assert_eq!(EventAction::AgentWrite.as_str(), "agent_write");
        assert_eq!(EventAction::Checkin.as_str(), "checkin");
        assert_eq!(EventAction::VideoSubmit.as_str(), "video_submit");
    }
}
