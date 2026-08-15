//! 刷题视图：练习范围（跨集综合练 / 单集 / 只练错题）下拉切换、进度条、
//! 即时判分与解析展示、⭐ 标记重点、下一题回绕。

use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    self, AnswerRequest, BTreeSetUsize, Chosen, DrawQuery, QuestionBrief, QuestionType,
};
use crate::state::{episode_map, scope_name, AppState, QuizScope};
use crate::views::ui::{
    close_all_dd, display_options, judge_chosen, opt_label, option_class, register_dd_closer,
    src_label, type_label,
};
use crate::views::wrong::refresh_wrong;

/// 按范围加载题目池并重置作答状态。
async fn load_scope(state: AppState, scope: QuizScope) {
    let Some(ws) = state.workspace.get_untracked() else {
        return;
    };
    let qs = match scope {
        QuizScope::All => {
            let pool = state.pool.get_untracked();
            if !pool.is_empty() {
                pool
            } else {
                api::draw(&DrawQuery {
                    workspace_id: ws.id,
                    scope: None,
                    count: Some(100),
                })
                .await
                .unwrap_or_default()
            }
        }
        QuizScope::Episode(c, e) => {
            let node_id = state
                .courses
                .get_untracked()
                .get(c)
                .and_then(|co| co.episodes.get(e))
                .map(|ep| ep.node_id);
            match node_id {
                Some(id) => api::draw(&DrawQuery {
                    workspace_id: ws.id,
                    scope: Some(id),
                    count: Some(100),
                })
                .await
                .unwrap_or_default(),
                None => Vec::new(),
            }
        }
        QuizScope::WrongOnly => api::wrong_list()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|w| w.question)
            .collect(),
    };
    let n = qs.len();
    state.quiz_pool.set(qs);
    state.quiz_idx.set(0);
    state.quiz_chosen.set(vec![None; n]);
    state.quiz_outcomes.set(vec![None; n]);
    state.quiz_answers.set(vec![None; n]);
    state.quiz_correct_cnt.set(0);
    state.quiz_wrong_cnt.set(0);
    state.quiz_badge.set(n as u32);
}

/// 当前范围的作答归属串：错题本 / 集名，跨集综合练不传 scope。
fn scope_string(state: AppState, scope: QuizScope) -> Option<String> {
    match scope {
        QuizScope::All => None,
        QuizScope::Episode(c, e) => {
            let courses = state.courses.get_untracked();
            Some(scope_name(&courses, c, e))
        }
        QuizScope::WrongOnly => Some("错题本".to_string()),
    }
}

fn submit_answer(state: AppState, idx: usize, q: &QuestionBrief, chosen: Chosen) {
    let qid = q.id;
    let scope = scope_string(state, state.quiz_scope.get_untracked());
    let st = state;
    spawn_local(async move {
        match api::answer(&AnswerRequest {
            question_id: qid,
            chosen: chosen.clone(),
            scope,
        })
        .await
        {
            Ok(out) => {
                let mut chosen_vec = st.quiz_chosen.get_untracked();
                chosen_vec[idx] = Some(chosen);
                st.quiz_chosen.set(chosen_vec);
                let mut outs = st.quiz_outcomes.get_untracked();
                outs[idx] = Some(out.clone());
                st.quiz_outcomes.set(outs);
                let mut ans = st.quiz_answers.get_untracked();
                ans[idx] = Some(out.answer.clone());
                st.quiz_answers.set(ans);
                if out.is_correct {
                    st.quiz_correct_cnt.update(|n| *n += 1);
                    st.toast("答对了，已记录");
                } else {
                    st.quiz_wrong_cnt.update(|n| *n += 1);
                    st.toast("答错了，已加入错题本");
                }
                refresh_wrong(st).await;
            }
            Err(e) => st.toast(&format!("作答失败：{}", e)),
        }
    });
}

/// 选项点击：单选 / 判断立即提交；多选先累积点选，由「确认答案」提交。
fn answer_click(state: AppState, q: QuestionBrief, i: usize, idx: usize) {
    if state
        .quiz_outcomes
        .get_untracked()
        .get(idx)
        .is_some_and(|o| o.is_some())
    {
        return;
    }
    if q.qtype == QuestionType::Multi {
        let mut chosen = state.quiz_chosen.get_untracked();
        let mut set = match chosen.get(idx).cloned().flatten() {
            Some(Chosen::Multi(s)) => s.0,
            _ => Vec::new(),
        };
        if let Some(p) = set.iter().position(|&x| x == i) {
            set.remove(p);
        } else {
            set.push(i);
        }
        chosen[idx] = Some(Chosen::Multi(BTreeSetUsize(set)));
        state.quiz_chosen.set(chosen);
        return;
    }
    let c = if q.qtype == QuestionType::Judge {
        judge_chosen(i)
    } else {
        Chosen::Single(i)
    };
    submit_answer(state, idx, &q, c);
}

fn submit_multi(state: AppState, idx: usize, q: &QuestionBrief) {
    let Some(Some(Chosen::Multi(set))) = state.quiz_chosen.get_untracked().get(idx).cloned() else {
        return;
    };
    if set.0.is_empty() {
        return;
    }
    submit_answer(state, idx, q, Chosen::Multi(set));
}

#[component]
pub fn QuizView(state: AppState) -> impl IntoView {
    let scope_open = RwSignal::new(false);
    let scope_search = RwSignal::new(String::new());
    let counts = RwSignal::new(HashMap::<(usize, usize), u32>::new());

    register_dd_closer(move || scope_open.set(false));

    // 范围切换 → 重新加载题目池。
    Effect::new(move |_| {
        let scope = state.quiz_scope.get();
        let st = state;
        spawn_local(async move {
            load_scope(st, scope).await;
        });
    });

    // 下拉首次打开时补齐各集题数。
    Effect::new(move |_| {
        if !scope_open.get() {
            return;
        }
        let st = state;
        let counts = counts;
        spawn_local(async move {
            let Some(ws) = st.workspace.get_untracked() else {
                return;
            };
            let courses = st.courses.get_untracked();
            let mut m = counts.get_untracked();
            for (c, co) in courses.iter().enumerate() {
                for (e, ep) in co.episodes.iter().enumerate() {
                    if m.contains_key(&(c, e)) {
                        continue;
                    }
                    let qs = api::draw(&DrawQuery {
                        workspace_id: ws.id,
                        scope: Some(ep.node_id),
                        count: Some(100),
                    })
                    .await
                    .unwrap_or_default();
                    m.insert((c, e), qs.len() as u32);
                    counts.set(m.clone());
                }
            }
        });
    });

    let scope_name_text = move || match state.quiz_scope.get() {
        QuizScope::All => "跨集综合练".to_string(),
        QuizScope::Episode(c, e) => {
            let courses = state.courses.get_untracked();
            scope_name(&courses, c, e)
        }
        QuizScope::WrongOnly => "只练错题".to_string(),
    };
    let scope_hint = move || format!("共 {} 题符合条件", state.quiz_pool.get().len());
    // P2-6：行内读数全部走 get()（订阅），题数与当前范围高亮随
    // quiz_scope / pool / counts / wrong_list 变化即时刷新。
    let scope_rows = move || {
        let cur = state.quiz_scope.get();
        let f = scope_search.get().trim().to_lowercase();
        let courses = state.courses.get();
        let mut rows: Vec<AnyView> = Vec::new();
        let matches = |name: &str| f.is_empty() || name.to_lowercase().contains(&f);

        if matches("跨集综合练") {
            let n = state.pool.get().len() as u32;
            let st = state;
            rows.push(
                (view! {
                    <div class="dd-item" class:active=cur == QuizScope::All
                        on:click=move |_| { st.quiz_scope.set(QuizScope::All); close_all_dd(); }>
                        <span>"跨集综合练"</span>
                        <span class="di-meta">{format!("{} 题", n)}</span>
                    </div>
                })
                .into_any(),
            );
        }
        for (c, co) in courses.iter().enumerate() {
            for (e, _ep) in co.episodes.iter().enumerate() {
                let name = scope_name(&courses, c, e);
                if !matches(&name) {
                    continue;
                }
                let n = counts.get().get(&(c, e)).copied().unwrap_or(0);
                let is_cur = cur == QuizScope::Episode(c, e);
                let st = state;
                rows.push((view! {
                    <div class="dd-item" class:active=is_cur
                        on:click=move |_| { st.quiz_scope.set(QuizScope::Episode(c, e)); close_all_dd(); }>
                        <span>{name}</span>
                        <span class="di-meta">{format!("{} 题", n)}</span>
                    </div>
                }).into_any());
            }
        }
        if matches("只练错题") {
            let n = state.wrong_list.get().len() as u32;
            let st = state;
            rows.push((view! {
                <div class="dd-item" class:active=cur == QuizScope::WrongOnly
                    on:click=move |_| { st.quiz_scope.set(QuizScope::WrongOnly); close_all_dd(); }>
                    <span>"只练错题"</span>
                    <span class="di-meta">{format!("{} 题", n)}</span>
                </div>
            }).into_any());
        }
        if rows.is_empty() {
            rows.push((view! {
                <div style="padding:14px 10px;font-size:12px;color:var(--muted-light)">"没有匹配的范围"</div>
            }).into_any());
        }
        rows
    };

    let card = move || {
        let idx = state.quiz_idx.get();
        let pool = state.quiz_pool.get();
        if pool.is_empty() {
            return (view! {
                <div class="quiz-card">
                    <div class="quiz-question">"该范围暂无题目，先去学习相关集数吧"</div>
                </div>
            })
            .into_any();
        }
        let Some(q) = pool.get(idx).cloned() else {
            return (view! { <div></div> }).into_any();
        };
        let courses = state.courses.get_untracked();
        let map = episode_map(&courses);
        let subject = map
            .get(&q.source_item_id)
            .and_then(|&(c, _)| courses.get(c))
            .map(|co| co.subject.clone())
            .unwrap_or_default();
        let src = src_label(&courses, &map, q.source_item_id);
        let qid = q.id;
        let subject_empty = subject.is_empty();
        let is_multi = q.qtype == QuestionType::Multi;

        let answered = move || {
            state
                .quiz_outcomes
                .get()
                .get(idx)
                .is_some_and(|o| o.is_some())
        };
        let multi_pending = move || {
            is_multi
                && state
                    .quiz_outcomes
                    .get()
                    .get(idx)
                    .is_some_and(|o| o.is_none())
                && state
                    .quiz_chosen
                    .get()
                    .get(idx)
                    .is_some_and(|c| c.is_some())
        };
        let exp = move || state.quiz_outcomes.get().get(idx).cloned().flatten();
        let ok_color = move || {
            if exp().as_ref().map(|o| o.is_correct).unwrap_or(true) {
                "var(--green)"
            } else {
                "var(--red)"
            }
        };
        let exp_label = move || {
            if exp().as_ref().map(|o| o.is_correct).unwrap_or(true) {
                "✓ 答对了 · 解析"
            } else {
                "✕ 再想想 · 解析"
            }
        };
        let exp_text = move || {
            exp()
                .and_then(|o| o.explanation)
                .unwrap_or_else(|| "暂无解析".to_string())
        };

        let options = display_options(&q)
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let q = q.clone();
                let chosen = move || state.quiz_chosen.get().get(idx).cloned().flatten();
                let out = move || state.quiz_outcomes.get().get(idx).cloned().flatten();
                (view! {
                    <div class=move || option_class(&chosen(), &out(), i, is_multi)
                        on:click=move |_| answer_click(state, q.clone(), i, idx)>
                        <div class="opt-label">{opt_label(i)}</div>
                        <span>{text}</span>
                    </div>
                })
                .into_any()
            })
            .collect::<Vec<_>>();

        let do_star = move |_| {
            let on = state.toggle_star(qid);
            state.toast(if on {
                "已标记为重点"
            } else {
                "已取消标记"
            });
        };
        let do_next = move |_| {
            let n = state.quiz_pool.get_untracked().len();
            if n == 0 {
                return;
            }
            let idx = state.quiz_idx.get_untracked();
            state.quiz_idx.set(if idx >= n - 1 { 0 } else { idx + 1 });
        };
        let q2 = q.clone();
        let do_confirm = move |_| {
            submit_multi(state, idx, &q2);
        };

        (view! {
            <div class="quiz-card">
                <div class="quiz-tag-row">
                    <Show when=move || state.stars.get().contains(&qid)>
                        <span class="quiz-tag star">"★ 考点"</span>
                    </Show>
                    <Show when=move || !subject_empty>
                        <span class="quiz-tag subject">{subject.clone()}</span>
                    </Show>
                    <span class="quiz-tag type">{type_label(&q)}</span>
                </div>
                <div class="quiz-question">{q.stem.clone()}</div>
                <div class="quiz-options">{options}</div>
                <div class="quiz-explanation" class:show=move || exp().is_some()
                    style:border-left-color=ok_color>
                    <div class="exp-label" style:color=ok_color>{exp_label}</div>
                    {exp_text}
                    <div class="exp-src">{format!("出自笔记：{}", src)}</div>
                </div>
                <div class="quiz-footer">
                    <button class="btn btn-ghost btn-sm" on:click=do_star>"⭐ 标记重点"</button>
                    <button class="btn btn-primary btn-sm"
                        style:display=move || if multi_pending() { "inline-block" } else { "none" }
                        on:click=do_confirm>"确认答案"</button>
                    <button class="btn btn-primary btn-sm"
                        style:visibility=move || if answered() { "visible" } else { "hidden" }
                        on:click=do_next>"下一题 →"</button>
                </div>
            </div>
        })
        .into_any()
    };

    view! {
        <div class="quiz-view">
            <div class="quiz-scope">
                <span class="scope-label">"练习范围"</span>
                <div class="dd" class:open=move || scope_open.get()>
                    <div class="dd-current" on:click=move |_| scope_open.set(!scope_open.get_untracked())>
                        <span>{scope_name_text}</span>
                        <span class="dd-caret">"▾"</span>
                    </div>
                    <div class="dd-panel">
                        <input
                            class="dd-search"
                            placeholder="搜索集数或范围…"
                            prop:value=move || scope_search.get()
                            on:input=move |ev| scope_search.set(event_target_value(&ev))
                            on:click=move |ev| ev.stop_propagation()
                        />
                        <div class="dd-list">{scope_rows}</div>
                    </div>
                </div>
                <span class="scope-hint">{scope_hint}</span>
            </div>

            <div class="src-hint">"💡 题库数据来源：AI 为每集笔记生成的习题 —— 例如「第1集 软件架构概念」生成的习题，即刷题时该集范围内的题目"</div>

            <Show when=move || !state.quiz_pool.get().is_empty()>
                <div class="quiz-progress-bar">
                    <div class="progress-track">
                        <div class="progress-fill"
                            style:width=move || {
                                let len = state.quiz_pool.get().len().max(1);
                                format!("{}%", (state.quiz_idx.get() + 1) * 100 / len)
                            }></div>
                    </div>
                    <div class="progress-text">
                        {move || format!("第 {} / {} 题", state.quiz_idx.get() + 1, state.quiz_pool.get().len())}
                    </div>
                </div>
            </Show>

            {card}
        </div>
    }
}
