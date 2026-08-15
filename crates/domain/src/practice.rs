//! 练习上下文：Question（含判分纯函数）、WrongItem、QuizRecord、Paper。

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// 题型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    Single,
    Multi,
    Judge,
}

impl QuestionType {
    pub fn as_str(self) -> &'static str {
        match self {
            QuestionType::Single => "single",
            QuestionType::Multi => "multi",
            QuestionType::Judge => "judge",
        }
    }
}

/// 题目标准答案（类型化；adapter 层负责与 jsonb 互转）。
///
/// 线格式遵循 docs/requirements.md §8.1：single → 数字（如 1），
/// multi → 索引数组（如 [0,2]），judge → 布尔（如 true），故用 untagged。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Answer {
    Single(usize),
    Multi(BTreeSet<usize>),
    Judge(bool),
}

/// 用户作答（类型化；REST 入参由 adapter 解析为 Chosen）。
///
/// 线格式与 Answer 相同（§8.1），untagged 保证作答请求体与判分序列化一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Chosen {
    Single(usize),
    Multi(BTreeSet<usize>),
    Judge(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judgment {
    pub is_correct: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: i64,
    pub workspace_id: i64,
    /// 归属的"集"：刷题范围与组卷筛选的唯一归属维度。
    pub source_item_id: i64,
    pub qtype: QuestionType,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: Answer,
    pub explanation: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Question {
    /// 作答判定领域纯函数：题型与所选/答案不匹配时报错，否则返回对错。
    pub fn judge(&self, chosen: &Chosen) -> Result<Judgment> {
        let is_correct = match (&self.qtype, chosen) {
            (QuestionType::Single, Chosen::Single(i)) => self.answer == Answer::Single(*i),
            (QuestionType::Multi, Chosen::Multi(s)) => self.answer == Answer::Multi(s.clone()),
            (QuestionType::Judge, Chosen::Judge(b)) => self.answer == Answer::Judge(*b),
            _ => {
                return Err(Error::Invalid(format!(
                    "作答类型与题目题型不匹配：题 {} 为 {}",
                    self.id,
                    self.qtype.as_str()
                )));
            }
        };
        Ok(Judgment { is_correct })
    }
}

/// 错题统计（错题本统计卡片）：累计错题数 / 近 7 天新增 / 已掌握。
/// 近 7 天新增按更新时间窗口近似（表无 created_at 列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrongStats {
    pub total: u32,
    pub weekly_new: u32,
    pub mastered: u32,
}

/// 错题聚合：同一题错多次记 times；用户显式标记 mastered。
/// 不变式：答错则 times += 1 且 mastered = false；重做答对不自动清除错题。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrongItem {
    pub id: i64,
    pub user_id: i64,
    pub question_id: i64,
    pub times: u32,
    pub mastered: bool,
    pub updated_at: DateTime<Utc>,
}

impl WrongItem {
    pub fn record_mistake(&mut self, now: DateTime<Utc>) {
        self.times += 1;
        self.mastered = false;
        self.updated_at = now;
    }

    pub fn mark_mastered(&mut self, now: DateTime<Utc>) {
        self.mastered = true;
        self.updated_at = now;
    }
}

/// 单次作答记录（只追加，不修改）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuizRecord {
    pub id: i64,
    pub user_id: i64,
    pub question_id: i64,
    /// 刷题范围快照（自由文本）。
    pub scope: Option<String>,
    /// 用户所选（类型化；adapter 序列化为 jsonb）。
    pub chosen: Option<Chosen>,
    pub is_correct: bool,
    pub created_at: DateTime<Utc>,
}

/// 组卷配置（类型化；adapter 序列化为 jsonb）：来源/题型/范围/数量。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperConfig {
    /// 范围限定（如"第 3 集"），自由文本快照。
    pub scope: Option<String>,
    pub question_types: Option<Vec<QuestionType>>,
    pub source_item_ids: Option<Vec<i64>>,
    pub count: u32,
}

/// 交卷结果：得分/正确率/用时。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperResult {
    pub score: u32,
    pub correct: u32,
    pub total: u32,
    pub duration_secs: u32,
}

impl PaperResult {
    pub fn accuracy(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.correct as f64 / self.total as f64
        }
    }
}

/// 试卷聚合：组卷是"筛选条件 + 题目快照"，抽题后题目列表冻结。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paper {
    pub id: i64,
    pub user_id: i64,
    pub workspace_id: i64,
    pub name: Option<String>,
    pub config: PaperConfig,
    /// 抽题快照（冻结的题目 id 顺序）。
    pub question_ids: Vec<i64>,
    pub result: Option<PaperResult>,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_q() -> Question {
        Question {
            id: 1,
            workspace_id: 1,
            source_item_id: 10,
            qtype: QuestionType::Single,
            stem: "1+1=?".into(),
            options: vec!["1".into(), "2".into(), "3".into()],
            answer: Answer::Single(1),
            explanation: None,
            created_at: Utc::now(),
        }
    }

    fn multi_q() -> Question {
        Question {
            id: 2,
            workspace_id: 1,
            source_item_id: 10,
            qtype: QuestionType::Multi,
            stem: "选偶数".into(),
            options: vec!["1".into(), "2".into(), "4".into()],
            answer: Answer::Multi([1, 2].into_iter().collect()),
            explanation: None,
            created_at: Utc::now(),
        }
    }

    fn judge_q() -> Question {
        Question {
            id: 3,
            workspace_id: 1,
            source_item_id: 10,
            qtype: QuestionType::Judge,
            stem: "地球是圆的".into(),
            options: vec![],
            answer: Answer::Judge(true),
            explanation: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn judge_single_correct_and_wrong() {
        let q = single_q();
        assert!(q.judge(&Chosen::Single(1)).unwrap().is_correct);
        assert!(!q.judge(&Chosen::Single(0)).unwrap().is_correct);
    }

    #[test]
    fn judge_multi_requires_exact_set() {
        let q = multi_q();
        // 全选对 → 对。
        assert!(
            q.judge(&Chosen::Multi([1, 2].into_iter().collect()))
                .unwrap()
                .is_correct
        );
        // 顺序无关 → 对。
        assert!(
            q.judge(&Chosen::Multi([2, 1].into_iter().collect()))
                .unwrap()
                .is_correct
        );
        // 漏选 → 错。
        assert!(
            !q.judge(&Chosen::Multi([1].into_iter().collect()))
                .unwrap()
                .is_correct
        );
        // 多选 → 错。
        assert!(
            !q.judge(&Chosen::Multi([0, 1, 2].into_iter().collect()))
                .unwrap()
                .is_correct
        );
    }

    #[test]
    fn judge_type_mismatch_is_error() {
        let q = single_q();
        assert!(matches!(
            q.judge(&Chosen::Judge(true)),
            Err(Error::Invalid(_))
        ));
        let j = judge_q();
        assert!(matches!(
            j.judge(&Chosen::Single(0)),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn judge_judge_q() {
        let q = judge_q();
        assert!(q.judge(&Chosen::Judge(true)).unwrap().is_correct);
        assert!(!q.judge(&Chosen::Judge(false)).unwrap().is_correct);
    }

    #[test]
    fn wrong_item_mistake_accumulates_and_resets_mastered() {
        let mut w = WrongItem {
            id: 1,
            user_id: 1,
            question_id: 1,
            times: 1,
            mastered: true,
            updated_at: Utc::now(),
        };
        let now = Utc::now();
        w.record_mistake(now);
        assert_eq!(w.times, 2);
        assert!(!w.mastered); // 答错重置掌握标记
    }

    #[test]
    fn wrong_item_master_is_explicit() {
        let mut w = WrongItem {
            id: 1,
            user_id: 1,
            question_id: 1,
            times: 3,
            mastered: false,
            updated_at: Utc::now(),
        };
        w.mark_mastered(Utc::now());
        assert!(w.mastered);
        // 重做答对不自动清除错题：没有该行为路径，mastered 只在显式标记时翻转。
        assert!(w.mastered);
    }

    #[test]
    fn paper_result_accuracy() {
        let r = PaperResult {
            score: 3,
            correct: 3,
            total: 5,
            duration_secs: 60,
        };
        assert!((r.accuracy() - 0.6).abs() < 1e-9);
        let empty = PaperResult {
            score: 0,
            correct: 0,
            total: 0,
            duration_secs: 0,
        };
        assert_eq!(empty.accuracy(), 0.0);
    }
}
