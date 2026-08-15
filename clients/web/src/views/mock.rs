//! 模考视图：信息栏（试卷名 / 题数 / 倒计时 / 交卷）、题目区（作答、上一题 /
//! 标记、下一题）、右侧答题卡（跳题 / 已答统计）。倒计时结束自动交卷。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Arc;

use gloo_timers::callback::Interval;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    self, AssembleRequest, BTreeSetUsize, Chosen, PaperAnswer, PaperConfig, QuestionBrief,
    QuestionType, SubmitRequest,
};
use crate::state::{
    fmt_duration, mock_duration_secs, AppState, ConfirmSpec, MockResult, MockSession, View,
};
use crate::views::ui::{display_options, judge_chosen, opt_label, option_class, type_label};
use crate::views::wrong::refresh_wrong;

// 模考定时器句柄（drop 即取消，须持有）与交卷防重入标记。
thread_local! {
    static MOCK_TIMER: RefCell<Option<Interval>> = const { RefCell::new(None) };
    static SUBMITTING: Cell<bool> = const { Cell::new(false) };
}

/// 🚀 开始模考：组卷接口按已选题目出卷 → 建会话 → 切到模考视图并启动倒计时。
pub fn start_mock(state: AppState) {
    let Some(ws) = state.workspace.get_untracked() else {
        state.toast("请先创建或选择备考空间");
        return;
    };
    let name = {
        let n = state.paper_name.get_untracked();
        if n.trim().is_empty() {
            let gen = format!("架构师模拟卷 #{}", state.next_paper_seq());
            state.paper_name.set(gen.clone());
            gen
        } else {
            n
        }
    };
    let ids: Vec<i64> = state.selected.get_untracked().into_iter().collect();
    let count = ids.len() as u32;
    let st = state;
    spawn_local(async move {
        let req = AssembleRequest {
            workspace_id: ws.id,
            name: Some(name.clone()),
            config: PaperConfig {
                scope: None,
                question_types: None,
                source_item_ids: Some(ids),
                count,
            },
        };
        match api::assemble_paper(&req).await {
            Ok(bundle) => {
                let n = bundle.questions.len();
                if n == 0 {
                    st.toast("所选题目组卷失败，请重新选择");
                    return;
                }
                let session = MockSession {
                    paper_id: bundle.paper.id,
                    name,
                    questions: bundle.questions,
                    answers: vec![None; n],
                    idx: 0,
                    marked: HashSet::new(),
                    start_ms: window().performance().map(|p| p.now()).unwrap_or(0.0),
                    remaining: mock_duration_secs(n as u32),
                };
                st.mock.set(Some(session));
                st.mock_result.set(None);
                st.paper_name.set(String::new());
                st.view.set(View::Mock);
                if let Some(el) = document().get_element_by_id("content-area") {
                    el.set_scroll_top(0);
                }
                start_timer(st);
            }
            Err(e) => st.toast(&format!("组卷失败：{}", e)),
        }
    });
}

/// 1 秒倒计时；归零时自动交卷。
fn start_timer(state: AppState) {
    stop_timer();
    let st = state;
    let timer = Interval::new(1000, move || {
        let Some(mut s) = st.mock.get_untracked() else {
            return;
        };
        if s.remaining <= 1 {
            s.remaining = 0;
            st.mock.set(Some(s));
            st.toast("考试时间到，自动交卷");
            submit_mock(st);
            return;
        }
        s.remaining -= 1;
        st.mock.set(Some(s));
    });
    MOCK_TIMER.with(|t| *t.borrow_mut() = Some(timer));
}

fn stop_timer() {
    MOCK_TIMER.with(|t| *t.borrow_mut() = None);
}

/// 交卷：收集作答提交后端，成功后弹结果、清会话并生成下一卷序号。
pub fn submit_mock(state: AppState) {
    if SUBMITTING.with(|f| f.replace(true)) {
        return;
    }
    stop_timer();
    let Some(session) = state.mock.get_untracked() else {
        SUBMITTING.with(|f| f.set(false));
        return;
    };
    let n = session.questions.len();
    let mut answers: Vec<PaperAnswer> = Vec::new();
    for (i, q) in session.questions.iter().enumerate() {
        if let Some(chosen) = session.answers.get(i).cloned().flatten() {
            answers.push(PaperAnswer {
                question_id: q.id,
                chosen,
            });
        }
    }
    let submitted = answers.len() as u32;
    let duration_secs = mock_duration_secs(n as u32) - session.remaining;
    let st = state;
    spawn_local(async move {
        match api::submit_paper(
            session.paper_id,
            &SubmitRequest {
                answers,
                duration_secs,
            },
        )
        .await
        {
            Ok(r) => {
                st.mock_result.set(Some(MockResult {
                    score: r.score,
                    correct: r.correct,
                    total: r.total,
                    duration_secs: r.duration_secs,
                    wrong_count: submitted.saturating_sub(r.correct),
                    skip_count: r.total.saturating_sub(submitted),
                }));
                st.mock.set(None);
                st.paper_name.set(String::new());
                refresh_wrong(st).await;
            }
            Err(e) => st.toast(&format!("交卷失败：{}", e)),
        }
        SUBMITTING.with(|f| f.set(false));
    });
}

/// 交卷确认：有未答题时弹确认框，否则直接交卷。
pub fn ask_submit_mock(state: AppState) {
    let Some(session) = state.mock.get_untracked() else {
        return;
    };
    let unanswered = session.answers.iter().filter(|a| a.is_none()).count();
    if unanswered > 0 {
        state.confirm.set(Some(ConfirmSpec {
            title: "交卷确认".to_string(),
            text_html: format!(
                "还有 <b style=\"color:var(--red)\">{}</b> 题未作答，确定要交卷吗？",
                unanswered
            ),
            ok_label: "仍要交卷".to_string(),
            on_ok: Arc::new(move || submit_mock(state)),
        }));
    } else {
        submit_mock(state);
    }
}

/// 选项点击：单选 / 判断直接覆盖作答，多选累积点选（交卷时统一提交）。
fn answer_click(state: AppState, q: &QuestionBrief, i: usize) {
    let Some(mut s) = state.mock.get_untracked() else {
        return;
    };
    let idx = s.idx;
    if q.qtype == QuestionType::Multi {
        let mut set = match s.answers.get(idx).cloned().flatten() {
            Some(Chosen::Multi(b)) => b.0,
            _ => Vec::new(),
        };
        if let Some(p) = set.iter().position(|&x| x == i) {
            set.remove(p);
        } else {
            set.push(i);
        }
        s.answers[idx] = if set.is_empty() {
            None
        } else {
            Some(Chosen::Multi(BTreeSetUsize(set)))
        };
    } else {
        let c = if q.qtype == QuestionType::Judge {
            judge_chosen(i)
        } else {
            Chosen::Single(i)
        };
        s.answers[idx] = Some(c);
    }
    state.mock.set(Some(s));
}

/// 翻题：边界提示（与原型一致）。
fn mock_prev(state: AppState) {
    let Some(mut s) = state.mock.get_untracked() else {
        return;
    };
    if s.idx == 0 {
        state.toast("已经是第 1 题");
        return;
    }
    s.idx -= 1;
    state.mock.set(Some(s));
}

fn mock_next(state: AppState) {
    let Some(mut s) = state.mock.get_untracked() else {
        return;
    };
    if s.idx >= s.questions.len() - 1 {
        state.toast("已经是最后一题，可以交卷了");
        return;
    }
    s.idx += 1;
    state.mock.set(Some(s));
}

#[component]
pub fn MockView(state: AppState) -> impl IntoView {
    let card = move || {
        let Some(s) = state.mock.get() else {
            return (view! {
                <div class="quiz-card"><div class="quiz-question">"模考未开始"</div></div>
            })
            .into_any();
        };
        let n = s.questions.len();
        if n == 0 {
            return (view! {
                <div class="quiz-card"><div class="quiz-question">"试卷为空"</div></div>
            })
            .into_any();
        }
        let idx = s.idx.min(n - 1);
        let Some(q) = s.questions.get(idx).cloned() else {
            return (view! { <div></div> }).into_any();
        };
        let courses = state.courses.get_untracked();
        let subject = crate::state::episode_map(&courses)
            .get(&q.source_item_id)
            .and_then(|&(c, _)| courses.get(c))
            .map(|co| co.subject.clone())
            .unwrap_or_default();
        let is_multi = q.qtype == QuestionType::Multi;
        let qnum = idx + 1;

        let options = display_options(&q)
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let q = q.clone();
                let chosen = move || {
                    state
                        .mock
                        .get()
                        .and_then(|s| s.answers.get(idx).cloned().flatten())
                };
                (view! {
                    <div class=move || option_class(&chosen(), &None, i, is_multi)
                        on:click=move |_| answer_click(state, &q, i)>
                        <div class="opt-label">{opt_label(i)}</div>
                        <span>{text}</span>
                    </div>
                })
                .into_any()
            })
            .collect::<Vec<_>>();

        let subject_empty = subject.is_empty();
        let marked = move || state.mock.get().is_some_and(|s| s.marked.contains(&s.idx));
        let do_mark = move |_| {
            let Some(mut s) = state.mock.get_untracked() else {
                return;
            };
            let on = s.marked.contains(&s.idx);
            if on {
                s.marked.remove(&s.idx);
            } else {
                s.marked.insert(s.idx);
            }
            state.mock.set(Some(s));
            state.toast(if on {
                "已取消标记"
            } else {
                "已标记本题"
            });
        };

        (view! {
            <div class="quiz-card">
                <div class="quiz-tag-row">
                    <span class="quiz-tag type">{format!("第 {} / {} 题", qnum, n)}</span>
                    <Show when=move || !subject_empty>
                        <span class="quiz-tag subject">{subject.clone()}</span>
                    </Show>
                    <span class="quiz-tag type">{type_label(&q)}</span>
                </div>
                <div class="quiz-question">{q.stem.clone()}</div>
                <div class="quiz-options">{options}</div>
                <div class="quiz-footer">
                    <button class="btn btn-ghost btn-sm" on:click=move |_| mock_prev(state)>"← 上一题"</button>
                    <button class="btn btn-ghost btn-sm" class:marked=marked on:click=do_mark>"🚩 标记"</button>
                    <button class="btn btn-primary btn-sm" on:click=move |_| mock_next(state)>"下一题 →"</button>
                </div>
            </div>
        }).into_any()
    };

    let sheet = move || {
        let Some(s) = state.mock.get() else {
            return (view! { <div></div> }).into_any();
        };
        let n = s.questions.len();
        if n == 0 {
            return (view! { <div></div> }).into_any();
        }
        let cells = (0..n)
            .map(|i| {
                let st = state;
                let answered = move || {
                    st.mock
                        .get()
                        .is_some_and(|s| s.answers.get(i).is_some_and(|a| a.is_some()))
                };
                let current = move || st.mock.get().is_some_and(|s| s.idx == i);
                (view! {
                    <div class="sheet-cell" class:answered=answered class:current=current
                        on:click=move |_| {
                            if let Some(mut s) = st.mock.get_untracked() {
                                if i < s.questions.len() {
                                    s.idx = i;
                                    st.mock.set(Some(s));
                                }
                            }
                        }>
                        {i + 1}
                    </div>
                })
                .into_any()
            })
            .collect::<Vec<_>>();
        let stat = move || {
            let answered = state
                .mock
                .get()
                .map(|s| s.answers.iter().filter(|a| a.is_some()).count())
                .unwrap_or(0);
            format!("已答 {} / {}", answered, n)
        };
        (view! {
            <div class="mock-sheet">
                <div class="sheet-title">"答题卡"</div>
                <div class="sheet-legend">
                    <span><i class="lg lg-empty"></i>"未答"</span>
                    <span><i class="lg lg-done"></i>"已答"</span>
                    <span><i class="lg lg-cur"></i>"当前"</span>
                </div>
                <div class="sheet-grid">{cells}</div>
                <div class="sheet-stat">{stat}</div>
            </div>
        })
        .into_any()
    };

    let title = move || {
        state
            .mock
            .get()
            .map(|s| format!("📜 {}", s.name))
            .unwrap_or_default()
    };
    let meta = move || {
        state
            .mock
            .get()
            .map(|s| {
                format!(
                    "{} 道题 · 满分 {} 分 · 倒计时结束自动交卷",
                    s.questions.len(),
                    s.questions.len()
                )
            })
            .unwrap_or_default()
    };
    let timer = move || {
        state
            .mock
            .get()
            .map(|s| format!("⏱ {}", fmt_duration(s.remaining)))
            .unwrap_or_default()
    };

    view! {
        <div class="mock-view">
            <div class="mock-info-bar">
                <div>
                    <div class="mock-title">{title}</div>
                    <div class="mock-meta">{meta}</div>
                </div>
                <div class="mock-timer">{timer}</div>
                <button class="btn btn-primary btn-sm" on:click=move |_| ask_submit_mock(state)>
                    "交卷"
                </button>
            </div>

            <div class="mock-layout">
                <div class="mock-main">{card}</div>
                {sheet}
            </div>
        </div>
    }
}
