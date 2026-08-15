//! 组卷视图：题目池（科目 / 来源 / 题型三组筛选 chips）、组卷面板（已选计数 /
//! 试卷名称 / 统计）、自动补齐至 75 题、试卷预览与开始模考确认。

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;

use crate::api::{QuestionBrief, QuestionType};
use crate::markdown::escape_html;
use crate::state::{
    episode_map, fmt_duration, mock_duration_secs, AppState, ConfirmSpec, PreviewPaper,
};

pub const TARGET: u32 = 75;

/// 题目学科标签（课程名推导，与笔记页一致）。
pub(crate) fn q_subject(state: AppState, q: &QuestionBrief) -> String {
    let courses = state.courses.get_untracked();
    episode_map(&courses)
        .get(&q.source_item_id)
        .and_then(|&(c, _)| courses.get(c))
        .map(|co| co.subject.clone())
        .unwrap_or_default()
}

/// 错题记录查询：question_id → 错误次数（含已掌握，语义与错题列表一致）。
pub(crate) fn wrong_times(state: AppState, qid: i64) -> Option<u32> {
    state
        .wrong_list
        .get_untracked()
        .iter()
        .find(|it| it.question.id == qid)
        .map(|it| it.wrong.times)
}

/// 题目来源标签：'第{n}集' / 'AI 生成'。
pub(crate) fn q_source(state: AppState, q: &QuestionBrief) -> String {
    match episode_map(&state.courses.get_untracked()).get(&q.source_item_id) {
        Some(&(_, e)) => format!("第{}集", e + 1),
        None => "AI 生成".to_string(),
    }
}

/// 三组筛选的通过判定（空集合 = 该组「全部」）。
fn passes(
    state: AppState,
    q: &QuestionBrief,
    subjects: &HashSet<String>,
    sources: &HashSet<String>,
    types: &HashSet<String>,
) -> bool {
    if !subjects.is_empty() && !subjects.contains(&q_subject(state, q)) {
        return false;
    }
    if !sources.is_empty() {
        let mut ok = false;
        if sources.contains("★ 考点") && state.is_starred(q.id) {
            ok = true;
        }
        if sources.contains("错题") && wrong_times(state, q.id).is_some() {
            ok = true;
        }
        if !ok {
            return false;
        }
    }
    if !types.is_empty() {
        let t = match q.qtype {
            QuestionType::Single => "单选",
            QuestionType::Multi => "多选",
            QuestionType::Judge => "判断",
        };
        if !types.contains(t) {
            return false;
        }
    }
    true
}

fn filtered_pool(
    state: AppState,
    subjects: &HashSet<String>,
    sources: &HashSet<String>,
    types: &HashSet<String>,
) -> Vec<QuestionBrief> {
    state
        .pool
        .get()
        .into_iter()
        .filter(|q| passes(state, q, subjects, sources, types))
        .collect()
}

/// 已选题目（按题库顺序）。
fn selected_questions(state: AppState) -> Vec<QuestionBrief> {
    let sel = state.selected.get_untracked();
    state
        .pool
        .get_untracked()
        .into_iter()
        .filter(|q| sel.contains(&q.id))
        .collect()
}

/// 已选统计：★考点 / 含错题 / 综合知识 / 案例分析。
pub(crate) fn cart_stats(state: AppState) -> (u32, u32, u32, u32) {
    // P1-4：订阅 selected / pool，勾选与题库加载后统计卡即时刷新
    let sel = state.selected.get();
    if sel.is_empty() {
        return (0, 0, 0, 0);
    }
    let mut star = 0u32;
    let mut wrong = 0u32;
    let mut comp = 0u32;
    let mut case = 0u32;
    for q in &state.pool.get() {
        if !sel.contains(&q.id) {
            continue;
        }
        if state.is_starred(q.id) {
            star += 1;
        }
        if wrong_times(state, q.id).is_some() {
            wrong += 1;
        }
        match q_subject(state, q).as_str() {
            "综合知识" => comp += 1,
            "案例分析" => case += 1,
            _ => {}
        }
    }
    (star, wrong, comp, case)
}

/// 当前试卷名（为空时生成下一个序号并持久化）。
fn paper_name_str(state: AppState) -> String {
    let n = state.paper_name.get_untracked();
    if n.trim().is_empty() {
        let gen = format!("架构师模拟卷 #{}", state.next_paper_seq());
        state.paper_name.set(gen.clone());
        gen
    } else {
        n
    }
}

/// ⚡ 自动补齐：★考点优先 + 错题优先，按题库顺序取至 75 题。
fn auto_fill(state: AppState) {
    let pool = state.pool.get_untracked();
    let mut sel = state.selected.get_untracked();
    if sel.len() as u32 >= TARGET {
        state.toast("已经是 75 题，无需补齐");
        return;
    }
    let mut candidates: Vec<&QuestionBrief> =
        pool.iter().filter(|q| !sel.contains(&q.id)).collect();
    candidates.sort_by_key(|q| (!state.is_starred(q.id), wrong_times(state, q.id).is_none()));
    for q in candidates {
        if sel.len() as u32 >= TARGET {
            break;
        }
        sel.insert(q.id);
    }
    let len = sel.len() as u32;
    state.selected.set(sel);
    if len >= TARGET {
        state.toast("已按「★考点优先 + 错题优先」策略补齐至 75 题");
    } else {
        state.toast(&format!("题库共 {} 题，已全部选中", len));
    }
}

/// 👁 预览试卷：按已选题目构建预览数据，交给弹窗渲染。
fn preview_paper(state: AppState) {
    let questions = selected_questions(state);
    let name = paper_name_str(state);
    state.preview.set(Some(PreviewPaper {
        name: name.clone(),
        questions,
        total: TARGET,
        score: TARGET,
        duration_secs: mock_duration_secs(TARGET),
    }));
    state.preview_open.set(true);
}

/// 🚀 开始模考：确认弹窗（空选择时直接提示）。
pub fn ask_start_mock(state: AppState) {
    let qs = selected_questions(state);
    if qs.is_empty() {
        state.toast("请先选择题目或使用「自动补齐」");
        return;
    }
    let n = qs.len() as u32;
    let name = paper_name_str(state);
    let text = format!(
        "<b>{}</b><br><br>\
         · 共 {} 道题，每题 1 分，满分 {} 分<br>\
         · 限时 {}，倒计时结束自动交卷<br>\
         · 可通过右侧答题卡跳题、标记题目，随时交卷<br>\
         · 交卷后自动判分，错题将进入错题本",
        escape_html(&name),
        n,
        n,
        fmt_duration(mock_duration_secs(n))
    );
    state.confirm.set(Some(ConfirmSpec {
        title: "开始模考".to_string(),
        text_html: text,
        ok_label: "开始考试".to_string(),
        on_ok: Arc::new(move || crate::views::mock::start_mock(state)),
    }));
}

#[component]
pub fn AssemblyView(state: AppState) -> impl IntoView {
    let subjects = RwSignal::new(HashSet::<String>::new());
    let sources = RwSignal::new(HashSet::<String>::new());
    let types = RwSignal::new(HashSet::<String>::new());

    // 试卷名缺省时生成 '架构师模拟卷 #N'（模考完成后重置回空，下一次自动递增）。
    Effect::new(move |_| {
        if state.paper_name.get().trim().is_empty() {
            state
                .paper_name
                .set(format!("架构师模拟卷 #{}", state.next_paper_seq()));
        }
    });

    /// 组内筛选 chip：'全部' 清空集合，其余 toggle。
    fn group_chip(set: RwSignal<HashSet<String>>, label: &'static str, state: AppState) -> AnyView {
        let is_all = label == "全部";
        let active = move || {
            let s = set.get();
            if is_all {
                s.is_empty()
            } else {
                s.contains(label)
            }
        };
        let st = state;
        (view! {
            <div class="filter-chip" class:active=active
                on:click=move |_| {
                    let mut s = set.get_untracked();
                    if is_all {
                        s.clear();
                    } else if s.contains(label) {
                        s.remove(label);
                    } else {
                        s.insert(label.to_string());
                    }
                    set.set(s);
                    st.toast("筛选已更新");
                }>
                {label}
            </div>
        },)
            .into_any()
    }

    let subject_chips = move || {
        let st = state;
        vec![
            group_chip(subjects, "全部", st),
            group_chip(subjects, "综合知识", st),
            group_chip(subjects, "案例分析", st),
        ]
    };
    let source_chips = move || {
        let st = state;
        vec![
            group_chip(sources, "全部", st),
            group_chip(sources, "★ 考点", st),
            group_chip(sources, "错题", st),
        ]
    };
    let type_chips = move || {
        let st = state;
        vec![
            group_chip(types, "全部", st),
            group_chip(types, "单选", st),
            group_chip(types, "判断", st),
        ]
    };

    let rows = move || {
        let list = filtered_pool(state, &subjects.get(), &sources.get(), &types.get());
        if list.is_empty() {
            return vec![(view! {
                <div style="padding:16px 10px;font-size:12.5px;color:var(--muted-light)">
                    "当前筛选下暂无题目"
                </div>
            },)
                .into_any()];
        }
        list.into_iter()
            .map(|q| {
                let st = state;
                let qid = q.id;
                let selected = move || st.selected.get().contains(&qid);
                let toggle = move |_| {
                    let mut sel = st.selected.get_untracked();
                    if sel.contains(&qid) {
                        sel.remove(&qid);
                    } else {
                        sel.insert(qid);
                    }
                    st.selected.set(sel);
                };
                let subject = q_subject(state, &q);
                let star = state.is_starred(qid);
                let wrong = wrong_times(state, qid);
                let source = q_source(state, &q);
                (view! {
                    <div class="pool-item" class:added=selected>
                        <div class="pool-checkbox" class:checked=selected on:click=toggle></div>
                        <div class="pool-content">
                            <div class="pool-question">{q.stem.clone()}</div>
                            <div class="pool-meta">
                                <span class="meta-tag subject">{subject}</span>
                                <Show when=move || star>
                                    <span class="meta-tag star">"★ 考点"</span>
                                </Show>
                                <span class="meta-tag source">{source}</span>
                                <Show when=move || wrong.is_some()>
                                    <span class="meta-tag wrong">
                                        {move || format!("错 {} 次", wrong.unwrap_or(0))}
                                    </span>
                                </Show>
                            </div>
                        </div>
                    </div>
                },)
                    .into_any()
            })
            .collect()
    };

    let selected_count = move || state.selected.get().len();
    let fill_width = move || {
        let n = state.selected.get().len() as f64;
        format!("{}%", (n / TARGET as f64 * 100.0).min(100.0))
    };
    let stats = move || cart_stats(state);

    view! {
        <div class="assembly-view">
            <div class="assembly-layout">
                <div class="assembly-main">
                    <div class="src-hint">"💡 组卷数据来源：从题库中的题目（AI 生成题 + 错题）筛选组合成模拟试卷"</div>
                    <div class="assembly-filters">
                        <span class="filter-label">"科目"</span>
                        {subject_chips}
                        <span class="filter-label" style="margin-left:14px;">"来源"</span>
                        {source_chips}
                        <span class="filter-label" style="margin-left:14px;">"题型"</span>
                        {type_chips}
                    </div>
                    <div class="question-pool">{rows}</div>
                </div>

                <div class="assembly-cart">
                    <div class="cart-title">"📜 组卷面板"</div>
                    <div class="cart-progress">
                        "已选 "
                        <span class="filled">{selected_count}</span>
                        " / 75 题"
                    </div>
                    <div class="progress-track">
                        <div class="progress-fill" style:width=fill_width></div>
                    </div>

                    <div class="cart-config">
                        <div class="cart-config-row">
                            <span class="label">"试卷名称"</span>
                            <span class="value">{move || state.paper_name.get()}</span>
                        </div>
                        <div class="cart-config-row">
                            <span class="label">"目标题数"</span>
                            <span class="value">"75 题"</span>
                        </div>
                        <div class="cart-config-row">
                            <span class="label">"满分"</span>
                            <span class="value">"75 分"</span>
                        </div>
                        <div class="cart-config-row">
                            <span class="label">"限时"</span>
                            <span class="value">"150 分钟"</span>
                        </div>
                    </div>

                    <div class="cart-stat-grid">
                        <div class="cart-stat">
                            <div class="num" style="color:var(--gold);">{move || stats().0}</div>
                            <div class="lbl">"★ 考点"</div>
                        </div>
                        <div class="cart-stat">
                            <div class="num" style="color:var(--red);">{move || stats().1}</div>
                            <div class="lbl">"含错题"</div>
                        </div>
                        <div class="cart-stat">
                            <div class="num" style="color:var(--accent);">{move || stats().2}</div>
                            <div class="lbl">"综合知识"</div>
                        </div>
                        <div class="cart-stat">
                            <div class="num" style="color:var(--muted);">{move || stats().3}</div>
                            <div class="lbl">"案例分析"</div>
                        </div>
                    </div>

                    <div class="cart-actions">
                        <button class="btn btn-primary" on:click=move |_| auto_fill(state)>
                            "⚡ 自动补齐至 75 题"
                        </button>
                        <button class="btn btn-ghost" on:click=move |_| preview_paper(state)>
                            "👁 预览试卷"
                        </button>
                        <button class="btn btn-ghost" on:click=move |_| ask_start_mock(state)>
                            "🚀 开始模考"
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}
