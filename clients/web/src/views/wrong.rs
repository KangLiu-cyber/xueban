//! 错题本视图：统计卡（累计 / 本周新增 / 已掌握）、筛选 chips、错题列表、
//! 单题重做与全部重刷。重做弹窗 UI 在 modals.rs 渲染，本文件提供状态流转
//! 与作答逻辑（redo_* 供弹窗调用）。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, AnswerRequest, BTreeSetUsize, Chosen, QuestionType, WrongListItem};
use crate::state::{episode_map, fmt_date_ymd, AppState, RedoState, WrongFilter};
use crate::views::ui::judge_chosen;

/// 拉取错题列表与统计，更新徽标（刷题 / 模考 / 重做后调用）。
pub async fn refresh_wrong(state: AppState) {
    if let Ok(list) = api::wrong_list().await {
        let n = list.len() as u32;
        state.wrong_badge.set(n);
        state.wrong_list.set(list);
    }
    if let Ok(stats) = api::wrong_stats().await {
        state.wrong_stats.set(Some(stats));
    }
}

/// 题目所属课程学科（综合知识 / 案例分析 …）。
fn subject_of(state: AppState, source_item_id: i64) -> String {
    let courses = state.courses.get_untracked();
    episode_map(&courses)
        .get(&source_item_id)
        .and_then(|&(c, _)| courses.get(c))
        .map(|co| co.subject.clone())
        .unwrap_or_default()
}

/// 客户端筛选：学科 / 重点标记 / 答错次数。
fn passes(state: AppState, f: WrongFilter, item: &WrongListItem) -> bool {
    match f {
        WrongFilter::All => true,
        WrongFilter::Comprehensive => subject_of(state, item.question.source_item_id) == "综合知识",
        WrongFilter::Case => subject_of(state, item.question.source_item_id) == "案例分析",
        WrongFilter::Starred => state.is_starred(item.question.id),
        WrongFilter::TwiceOrMore => item.wrong.times >= 2,
    }
}

fn filtered(state: AppState, f: WrongFilter) -> Vec<WrongListItem> {
    state
        .wrong_list
        .get_untracked()
        .into_iter()
        .filter(|it| passes(state, f, it))
        .collect()
}

fn filter_count(state: AppState, f: WrongFilter) -> usize {
    state
        .wrong_list
        .get_untracked()
        .iter()
        .filter(|it| passes(state, f, it))
        .count()
}

// ---- 重做 ----

/// 开始重做：单题（列表「重做」按钮）或全部（「开始重刷错题」）。
pub fn redo_start(state: AppState, items: Vec<WrongListItem>) {
    let n = items.len();
    if n == 0 {
        state.toast("没有可重做的错题");
        return;
    }
    state.redo_list.set(items);
    state.redo_idx.set(0);
    state.redo_state.set(vec![
        RedoState {
            outcome: None,
            picked: None,
            mastered: false
        };
        n
    ]);
    state.redo_open.set(true);
}

fn redo_wrong(state: AppState, item: WrongListItem) {
    redo_start(state, vec![item]);
}

/// 选项点击：单选 / 判断立即作答；多选累积点选，由「确认答案」提交。
pub fn redo_toggle(state: AppState, opt: usize) {
    let idx = state.redo_idx.get_untracked();
    if state
        .redo_state
        .get_untracked()
        .get(idx)
        .is_some_and(|s| s.outcome.is_some())
    {
        return;
    }
    let Some(item) = state.redo_list.get_untracked().get(idx).cloned() else {
        return;
    };
    let q = item.question;
    if q.qtype == QuestionType::Multi {
        let mut sts = state.redo_state.get_untracked();
        let mut picked = match sts.get(idx).and_then(|s| s.picked.clone()) {
            Some(Chosen::Multi(set)) => set.0,
            _ => Vec::new(),
        };
        if let Some(p) = picked.iter().position(|&x| x == opt) {
            picked.remove(p);
        } else {
            picked.push(opt);
        }
        if let Some(s) = sts.get_mut(idx) {
            s.picked = Some(Chosen::Multi(BTreeSetUsize(picked)));
        }
        state.redo_state.set(sts);
        return;
    }
    let c = if q.qtype == QuestionType::Judge {
        judge_chosen(opt)
    } else {
        Chosen::Single(opt)
    };
    redo_submit(state, c);
}

/// 多选「确认答案」提交。
pub fn redo_confirm(state: AppState) {
    let idx = state.redo_idx.get_untracked();
    let Some(Chosen::Multi(set)) = state
        .redo_state
        .get_untracked()
        .get(idx)
        .and_then(|s| s.picked.clone())
    else {
        return;
    };
    if set.0.is_empty() {
        return;
    }
    redo_submit(state, Chosen::Multi(set));
}

fn redo_submit(state: AppState, chosen: Chosen) {
    let idx = state.redo_idx.get_untracked();
    let Some(item) = state.redo_list.get_untracked().get(idx).cloned() else {
        return;
    };
    let qid = item.question.id;
    let st = state;
    spawn_local(async move {
        match api::answer(&AnswerRequest {
            question_id: qid,
            chosen: chosen.clone(),
            scope: Some("错题本".to_string()),
        })
        .await
        {
            Ok(out) => {
                let mut sts = st.redo_state.get_untracked();
                if let Some(s) = sts.get_mut(idx) {
                    s.picked = Some(chosen);
                    s.outcome = Some(out.clone());
                    s.mastered = out.is_correct;
                }
                st.redo_state.set(sts);
                if out.is_correct {
                    st.toast("答对了，已移出错题本");
                    // P0-3：后端按 question_id 定位错题，传题目 id 而非 wrong.id
                    let _ = api::mark_mastered(qid).await;
                } else {
                    st.toast("又答错了，保留在错题本");
                }
                refresh_wrong(st).await;
            }
            Err(e) => st.toast(&format!("作答失败：{}", e)),
        }
    });
}

/// 下一题；已是最后一题 → idx 越界，弹窗据此切到总结页。
pub fn redo_next(state: AppState) {
    let n = state.redo_list.get_untracked().len();
    let idx = state.redo_idx.get_untracked();
    if n == 0 {
        return;
    }
    state.redo_idx.set(if idx >= n - 1 { n } else { idx + 1 });
}

pub fn redo_exit(state: AppState) {
    state.redo_open.set(false);
}

// ---- 视图 ----

#[component]
pub fn WrongView(state: AppState) -> impl IntoView {
    let chips = [
        WrongFilter::All,
        WrongFilter::Comprehensive,
        WrongFilter::Case,
        WrongFilter::Starred,
        WrongFilter::TwiceOrMore,
    ];

    let chip_row = move || {
        let cur = state.wrong_filter.get();
        chips
            .into_iter()
            .map(|f| {
                let n = filter_count(state, f);
                let st = state;
                (view! {
                    <div class="filter-chip" class:active=cur == f
                        on:click=move |_| {
                            st.wrong_filter.set(f);
                            st.toast(&format!("已筛选：{} {} 题", f.label(), n));
                        }>
                        {f.label()}
                    </div>
                })
                .into_any()
            })
            .collect::<Vec<_>>()
    };

    let rows = move || {
        let items = filtered(state, state.wrong_filter.get());
        if items.is_empty() {
            return vec![(view! {
                <div style="padding:16px 10px;font-size:12.5px;color:var(--muted-light)">
                    "暂无错题，继续刷题吧"
                </div>
            })
            .into_any()];
        }
        let courses = state.courses.get_untracked();
        let map = episode_map(&courses);
        items
            .into_iter()
            .map(|item| {
                let q = item.question.clone();
                let st = state;
                let meta = match map.get(&q.source_item_id) {
                    Some(&(c, e)) => {
                        let subject = courses
                            .get(c)
                            .map(|co| co.subject.clone())
                            .unwrap_or_default();
                        let ep = format!("第{}集", e + 1);
                        if subject.is_empty() {
                            ep
                        } else {
                            format!("{} · {}", subject, ep)
                        }
                    }
                    None => "AI 生成".to_string(),
                };
                let date = fmt_date_ymd(item.wrong.updated_at);
                let mastered = item.wrong.mastered;
                let count_label = if mastered {
                    "已掌握".to_string()
                } else {
                    format!("错 {} 次", item.wrong.times)
                };
                let it = item;
                let it2 = it.clone();
                (view! {
                    <div class="wrong-item" class:mastered=mastered
                        on:click=move |_| redo_wrong(st, it.clone())>
                        <div class="wrong-icon">{if mastered { "✓" } else { "✕" }}</div>
                        <div class="wrong-content">
                            <div class="wrong-question">{q.stem.clone()}</div>
                            <div class="wrong-meta">
                                <span>{meta}</span>
                                <span>{date}</span>
                                <span class="wrong-count">{count_label}</span>
                            </div>
                        </div>
                        <button class="btn btn-ghost btn-sm"
                            on:click=move |ev| { ev.stop_propagation(); redo_wrong(st, it2.clone()); }>
                            "重做"
                        </button>
                    </div>
                }).into_any()
            })
            .collect()
    };

    let stat = move |sel: &'static str, label: &'static str| {
        let s = state.wrong_stats.get();
        let n = match sel {
            "danger" => s.as_ref().map(|x| x.total).unwrap_or(0),
            "warning" => s.as_ref().map(|x| x.weekly_new).unwrap_or(0),
            _ => s.as_ref().map(|x| x.mastered).unwrap_or(0),
        };
        (view! {
            <div class=format!("stat-card {}", sel)>
                <div class="stat-num">{n}</div>
                <div class="stat-label">{label}</div>
            </div>
        })
        .into_any()
    };

    view! {
        <div class="wrong-view">
            <div class="wrong-header">
                <div class="wrong-stats">
                    {move || stat("danger", "累计错题")}
                    {move || stat("warning", "本周新增")}
                    {move || stat("ok", "已掌握")}
                </div>
                <button class="btn btn-primary btn-sm"
                    on:click=move |_| redo_start(state, state.wrong_list.get_untracked())>
                    "开始重刷错题"
                </button>
            </div>

            <div class="src-hint">"💡 错题本数据来源：刷题 / 模考过程中的答错记录自动归集（使用过程数据）"</div>

            <div class="assembly-filters" style="margin-bottom:16px;">
                <span class="filter-label">"筛选"</span>
                {chip_row}
            </div>

            <div class="wrong-list">{rows}</div>
        </div>
    }
}
