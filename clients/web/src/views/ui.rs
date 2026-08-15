//! UI 辅助：下拉关闭注册表 / 视图切换钩子 / 题型与选项标签工具。

use std::cell::RefCell;

use crate::api::{Chosen, QuestionBrief, QuestionType};
use crate::state::Course;

thread_local! {
    static DD_CLOSERS: RefCell<Vec<Box<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
    static VIEW_SWITCH_HOOKS: RefCell<Vec<Box<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

/// 注册一个「点击外部时关闭」的下拉；返回后立即调用可关闭全部。
pub fn register_dd_closer(f: impl Fn() + 'static) {
    DD_CLOSERS.with(|c| c.borrow_mut().push(Box::new(f)));
}

pub fn close_all_dd() {
    DD_CLOSERS.with(|c| {
        let closers = std::mem::take(&mut *c.borrow_mut());
        for f in closers.iter() {
            f();
        }
        *c.borrow_mut() = closers;
    });
}

pub fn register_view_switch_hook(f: impl Fn() + 'static) {
    VIEW_SWITCH_HOOKS.with(|c| c.borrow_mut().push(Box::new(f)));
}

pub fn fire_view_switch_hooks() {
    VIEW_SWITCH_HOOKS.with(|c| {
        let hooks = std::mem::take(&mut *c.borrow_mut());
        for f in hooks.iter() {
            f();
        }
        *c.borrow_mut() = hooks;
    });
}

/// 选项标签：A–Z。
pub fn opt_label(idx: usize) -> char {
    (b'A' + idx as u8).clamp(b'A', b'Z') as char
}

pub fn type_label(q: &QuestionBrief) -> &'static str {
    match q.qtype {
        QuestionType::Single => "单选题",
        QuestionType::Multi => "多选题",
        QuestionType::Judge => "判断题",
    }
}

/// 题型短标签（组卷筛选 chips 用，原型为「单选 / 多选 / 判断」）。
pub fn type_label_short(q: &QuestionBrief) -> &'static str {
    match q.qtype {
        QuestionType::Single => "单选",
        QuestionType::Multi => "多选",
        QuestionType::Judge => "判断",
    }
}

/// 判断题选项兜底：后端可能不生成「正确/错误」选项文本。
pub fn display_options(q: &QuestionBrief) -> Vec<String> {
    if q.qtype == QuestionType::Judge && q.options.is_empty() {
        vec!["错误".to_string(), "正确".to_string()]
    } else {
        q.options.clone()
    }
}

/// 判断题按下标（0=错误 1=正确）构造作答。
pub fn judge_chosen(idx: usize) -> Chosen {
    Chosen::Judge(idx == 1)
}

/// 题目来源标签：'第{n}集 {title}' / '{course} 第{n}集 {title}' / 'AI 生成' / '错题本'。
pub fn src_label(
    courses: &[Course],
    map: &std::collections::HashMap<i64, (usize, usize)>,
    source_item_id: i64,
) -> String {
    match map.get(&source_item_id) {
        Some(&(c, e)) => crate::state::scope_name(courses, c, e),
        None => "AI 生成".to_string(),
    }
}

/// 选项点击后的视觉态：未作答 / 已选 / 正确 / 错误。
pub fn option_class(
    chosen: &Option<Chosen>,
    outcome: &Option<crate::api::AnswerOutcome>,
    idx: usize,
    is_multi: bool,
) -> &'static str {
    let mut cls = "quiz-option";
    let selected = match chosen {
        Some(Chosen::Single(i)) => *i == idx,
        Some(Chosen::Judge(b)) => (idx == 1) == *b,
        Some(Chosen::Multi(set)) => set.0.contains(&idx),
        None => false,
    };
    if selected {
        cls = "quiz-option selected";
    }
    if let Some(out) = outcome {
        cls = "quiz-option disabled";
        let is_correct = match &out.answer {
            Chosen::Single(i) => *i == idx,
            Chosen::Judge(b) => (idx == 1) == *b,
            Chosen::Multi(set) => set.0.contains(&idx),
        };
        if is_correct {
            cls = "quiz-option correct";
        } else if selected {
            cls = "quiz-option wrong";
        }
        let _ = is_multi;
    }
    cls
}
