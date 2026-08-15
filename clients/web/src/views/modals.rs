//! 全局弹窗层：确认 / 试卷预览 / 错题重做 / 模考结果 / 考试目标设置 / Agent 接入 /
//! 批注编辑 / 批注详情 / 批注悬浮按钮 / Toast。由 Shell 挂载一次，随状态信号开合。

use chrono::{NaiveDate, Utc};
use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::{Closure, JsValue};
use wasm_bindgen::JsCast;

use crate::api::{self, QuestionType, Workspace, WorkspaceInput};
use crate::state::{episode_map, fmt_date_ymd, fmt_duration, AppState, View};
use crate::views::assembly::{
    ask_start_mock, cart_stats, q_source, q_subject, wrong_times, TARGET,
};
use crate::views::notes::annotate_from_fab;
use crate::views::shell::{init_data, switch_view};
use crate::views::ui::{display_options, opt_label, option_class, src_label, type_label};
use crate::views::wrong::{redo_confirm, redo_exit, redo_next, redo_toggle};

/// 复制成功后的收尾：toast + 关闭 Agent 弹窗。
fn copy_done(state: AppState) {
    state.toast("接入凭证已复制，发送给任意 Agent 即可完成接入");
    state.agent_open.set(false);
}

/// 剪贴板 API 不可用 / 被拒时的降级复制（隐藏 textarea + execCommand）。
fn fallback_copy(state: AppState, text: &str) {
    let doc = document();
    let Ok(ta) = doc.create_element("textarea") else {
        return;
    };
    let ta = ta.unchecked_into::<web_sys::HtmlTextAreaElement>();
    let _ = web_sys::HtmlElement::style(&ta).set_property("position", "fixed");
    let _ = web_sys::HtmlElement::style(&ta).set_property("opacity", "0");
    ta.set_value(text);
    let Some(body) = doc.body() else {
        return;
    };
    let _ = body.append_child(&ta);
    ta.select();
    let ok = doc
        .unchecked_into::<web_sys::HtmlDocument>()
        .exec_command("copy")
        .unwrap_or(false);
    let _ = body.remove_child(&ta);
    if ok {
        copy_done(state);
    } else {
        state.toast("自动复制失败，请在弹框中手动全选复制");
    }
}

/// 复制接入凭证：优先 navigator.clipboard，失败降级 exec_command。
fn copy_credential(state: AppState, text: String) {
    let clip = window().navigator().clipboard();
    let p = clip.write_text(&text);
    let on_ok =
        Closure::wrap(Box::new(move |_: JsValue| copy_done(state)) as Box<dyn FnMut(JsValue)>);
    let st = state;
    let on_err = Closure::wrap(
        Box::new(move |_: JsValue| fallback_copy(st, &text)) as Box<dyn FnMut(JsValue)>
    );
    let _ = p.then(&on_ok);
    let _ = p.catch(&on_err);
    on_ok.forget();
    on_err.forget();
}

#[component]
pub fn Modals(state: AppState) -> impl IntoView {
    // ---- Toast：nonce 防串台，2200ms 后若无新 toast 才隐藏 ----
    let toast_msg = move || state.toast.get().map(|(m, _)| m).unwrap_or_default();
    let toast_show = move || state.toast.get().is_some();
    Effect::new(move |_| {
        let Some((_, nonce)) = state.toast.get() else {
            return;
        };
        let st = state;
        let t = Timeout::new(2200, move || {
            if st.toast.get_untracked().map(|(_, n)| n) == Some(nonce) {
                st.toast.set(None);
            }
        });
        t.forget();
    });

    // ---- 考试目标设置：打开时预填 ----
    let setup_goal = RwSignal::new(String::new());
    let setup_date = RwSignal::new(String::new());
    Effect::new(move |_| {
        if !state.setup_open.get() {
            return;
        }
        let ws = state.workspace.get_untracked();
        setup_goal.set(ws.as_ref().map(|w| w.exam_goal.clone()).unwrap_or_default());
        setup_date.set(
            ws.as_ref()
                .and_then(|w| w.exam_date)
                .map(|d| d.to_string())
                .unwrap_or_default(),
        );
    });
    let save_setup = move |_| {
        let g = setup_goal.get_untracked().trim().to_string();
        if g.is_empty() {
            state.toast("请手写填写你的考试目标");
            return;
        }
        let d = setup_date.get_untracked();
        let exam_date = if d.trim().is_empty() {
            None
        } else {
            NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()
        };
        let input = WorkspaceInput {
            name: g.clone(),
            exam_goal: g,
            exam_date,
        };
        let st = state;
        spawn_local(async move {
            match st.workspace.get_untracked() {
                Some(ws) => match api::update_workspace(ws.id, &input).await {
                    Ok(w) => {
                        st.set_workspace(&w);
                        st.setup_open.set(false);
                        st.toast("考试目标与日期已更新");
                    }
                    Err(e) => st.toast(&format!("保存失败：{}", e)),
                },
                None => match api::create_workspace(&input).await {
                    Ok(w) => {
                        st.set_workspace(&w);
                        st.setup_open.set(false);
                        st.toast(&format!("已创建备考空间「{}」", input.name));
                        // P0-2：兜底恢复登录用户（正常流程登录时已置 user）
                        if st.user.get_untracked().is_none() {
                            let u = crate::state::ls_get(crate::state::LS_USER)
                                .and_then(|s| serde_json::from_str(&s).ok());
                            if let Some(u) = u {
                                st.user.set(Some(u));
                            }
                        }
                        init_data(st).await;
                        let t = Timeout::new(700, move || st.agent_open.set(true));
                        t.forget();
                    }
                    Err(e) => st.toast(&format!("创建失败：{}", e)),
                },
            }
        });
    };

    // ---- Agent 接入：打开时拉取凭证文本 ----
    Effect::new(move |_| {
        if !state.agent_open.get() {
            return;
        }
        let st = state;
        spawn_local(async move {
            // P1-6：首次获取 404（尚无 agent token）时自动 rotate 签发
            let cred = match api::credential().await {
                Ok(c) => Ok(c),
                Err(api::ApiError::Http(404, _)) => api::rotate_credential().await,
                Err(e) => Err(e),
            };
            match cred {
                Ok(c) => {
                    let name = st
                        .user
                        .get_untracked()
                        .map(|u| u.nickname.unwrap_or(u.account))
                        .unwrap_or_default();
                    let goal = st
                        .workspace
                        .get_untracked()
                        .map(|w| w.exam_goal)
                        .unwrap_or_default();
                    st.agent_text.set(format!(
                        "【超级学习助手 · Agent 接入凭证】\n请代我接入以下 MCP 服务并完成装配：\n　MCP 端点：{}\n　用户凭证：{}\n　绑定用户：{}（考试目标：{}）\n接入后，服务会自动下发：\n　· Skill：笔记生成 / 习题生成 / 复盘分析\n　· 提示词：备考场景专用提示词\n　· MCP 工具：读写目录、笔记、批注、习题，读取错题与答题事件\n装配完成后，请以我的名义与本系统交互（生成内容写入我的备考空间）。",
                        c.endpoint, c.token, name, goal
                    ));
                }
                Err(e) => st.toast(&format!("获取接入凭证失败：{}", e)),
            }
        });
    });

    // ---- P2-10：切换备考空间 ----
    let switch_workspace = move |ws: Workspace| {
        // 模考会话绑定旧空间：先停表清会话再换空间，防止错卷串空间。
        crate::views::mock::pause_timer();
        state.mock.set(None);
        crate::state::ls_remove(crate::state::LS_MOCK_PAPER);
        state.set_workspace(&ws);
        state.episode.set(None);
        state.setup_open.set(false);
        state.toast(&format!("已切换到备考空间「{}」", ws.name));
        let st = state;
        spawn_local(async move {
            init_data(st).await;
        });
    };
    let ws_rows = move || {
        let cur_id = state.workspace.get().map(|w| w.id);
        let mut rows: Vec<AnyView> = state
            .workspaces
            .get()
            .into_iter()
            .map(|ws| {
                let is_cur = cur_id == Some(ws.id);
                let st = state;
                (view! {
                    <div class="ws-row" class:active=is_cur
                        on:click=move |_| {
                            if st.workspace.get_untracked().map(|w| w.id) == Some(ws.id) {
                                return;
                            }
                            switch_workspace(ws.clone());
                        }>
                        <div class="ws-row-main">
                            <div class="ws-row-name">{ws.name.clone()}</div>
                            <div class="ws-row-goal">{ws.exam_goal.clone()}</div>
                        </div>
                        <span class="ws-row-meta">
                            {move || {
                                if is_cur {
                                    "当前".to_string()
                                } else {
                                    ws.exam_date
                                        .map(|d| format!("倒计时 {} 天", crate::state::exam_days_left(Some(d))))
                                        .unwrap_or_else(|| "未设考试日期".to_string())
                                }
                            }}
                        </span>
                    </div>
                })
                .into_any()
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            rows.push((
                view! {
                    <div style="padding:10px 12px;font-size:12px;color:var(--muted-light)">"暂无备考空间"</div>
                },
            ).into_any());
        }
        rows
    };

    // ---- 确认弹窗 ----
    let confirm_modal = move || {
        (view! {
            <Show when=move || state.confirm.get().is_some()>
                <div class="modal-overlay" class:show=move || state.confirm.get().is_some()>
                    <div class="modal" style="width:440px;">
                        <div class="modal-head">
                            <div class="modal-title">{move || state.confirm.get().map(|s| s.title).unwrap_or_default()}</div>
                            <button class="modal-close" on:click=move |_| state.confirm.set(None)>"✕"</button>
                        </div>
                        <div class="modal-body"
                            inner_html=move || state.confirm.get().map(|s| s.text_html).unwrap_or_default()></div>
                        <div class="modal-foot">
                            <button class="btn btn-ghost" on:click=move |_| state.confirm.set(None)>"取消"</button>
                            <button class="btn btn-primary"
                                on:click=move |_| {
                                    let Some(s) = state.confirm.get_untracked() else { return; };
                                    state.confirm.set(None);
                                    (s.on_ok)();
                                }>
                                {move || state.confirm.get().map(|s| s.ok_label).unwrap_or_default()}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 试卷预览 ----
    let preview_modal = move || {
        (view! {
            <Show when=move || state.preview_open.get() && state.preview.get().is_some()>
                <div class="modal-overlay" class:show=move || state.preview_open.get() && state.preview.get().is_some()>
                    <div class="modal" style="width:680px;">
                        <div class="modal-head">
                            <div>
                                <div class="modal-title">{move || format!("👁 试卷预览 · {}", state.preview.get().map(|p| p.name).unwrap_or_default())}</div>
                                <div class="modal-sub">
                                    {move || {
                                        let Some(p) = state.preview.get() else { return String::new() };
                                        format!(
                                            "{} 题 · 满分 {} 分 · 限时 {} · 生成于 {}",
                                            p.total, p.score, fmt_duration(p.duration_secs), fmt_date_ymd(Utc::now())
                                        )
                                    }}
                                </div>
                            </div>
                            <button class="modal-close" on:click=move |_| state.preview_open.set(false)>"✕"</button>
                        </div>
                        <div class="modal-body">
                            <div class="pv-stats">
                                <span class="pv-stat"><b>{move || cart_stats(state).2}</b>综合知识</span>
                                <span class="pv-stat"><b>{move || cart_stats(state).3}</b>案例分析</span>
                                <span class="pv-stat"><b>{move || cart_stats(state).0}</b>"★ 考点"</span>
                                <span class="pv-stat"><b>{move || cart_stats(state).1}</b>含错题</span>
                            </div>
                            {move || {
                                let Some(p) = state.preview.get() else {
                                    return (view! { <div></div> }).into_any();
                                };
                                let qs = p.questions;
                                let n = qs.len();
                                let rows = qs.iter().take(TARGET as usize).enumerate().map(|(i, q)| {
                                    let q = q.clone();
                                    let subject = q_subject(state, &q);
                                    let star = state.is_starred(q.id);
                                    let source = q_source(state, &q);
                                    let wrong_label = wrong_times(state, q.id)
                                        .map(|t| format!("错 {} 次", t))
                                        .unwrap_or_default();
                                    let has_wrong = wrong_times(state, q.id).is_some();
                                    (view! {
                                        <div class="pv-row">
                                            <div class="pv-num">{i + 1}</div>
                                            <div class="pv-q">
                                                {q.stem.clone()}
                                                <div class="pv-tags">
                                                    <span class="meta-tag subject">{subject}</span>
                                                    <Show when=move || star>
                                                        <span class="meta-tag star">"★ 考点"</span>
                                                    </Show>
                                                    <span class="meta-tag source">{source}</span>
                                                    <Show when=move || has_wrong>
                                                        <span class="meta-tag wrong">{wrong_label.clone()}</span>
                                                    </Show>
                                                </div>
                                            </div>
                                        </div>
                                    }).into_any()
                                }).collect::<Vec<_>>();
                                let more = if n == 0 {
                                    "还没有选择任何题目，请先在左侧勾选题目或使用「自动补齐」".to_string()
                                } else if n >= TARGET as usize {
                                    format!("预览显示前 {} 题，共 {} 题", TARGET, TARGET)
                                } else {
                                    format!("当前已选 {} 题，还差 {} 题", n, TARGET - n as u32)
                                };
                                let filled = n >= TARGET as usize;
                                view! {
                                    <div class="pv-rows">{rows}</div>
                                    <div class="pv-more">{more}</div>
                                    <div class="pv-hint" class:ok=filled>
                                        {if filled {
                                            format!("✓ 共 {} 题 · 组卷完成，可以开始模考", TARGET)
                                        } else {
                                            format!("还差 {} 题 · 可使用「自动补齐」按 ★考点优先 + 错题优先 策略补足", TARGET - n as u32)
                                        }}
                                    </div>
                                }.into_any()
                            }}
                        </div>
                        <div class="modal-foot">
                            <button class="btn btn-ghost"
                                on:click=move |_| {
                                    let name = state.preview.get().map(|p| p.name).unwrap_or_default();
                                    state.toast(&format!("已导出「{}.pdf」（演示）", name));
                                }>
                                "📥 导出试卷 (PDF)"
                            </button>
                            <button class="btn btn-primary"
                                on:click=move |_| {
                                    state.preview_open.set(false);
                                    ask_start_mock(state);
                                }>
                                "🚀 开始模考"
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 错题重做 ----
    let redo_modal = move || {
        let open = state.redo_open.get();
        let n = state.redo_list.get_untracked().len();
        let show = open && n > 0;
        (view! {
            <Show when=move || show>
                <div class="modal-overlay" class:show=move || show>
                    <div class="modal" style="width:560px;">
                        <div class="modal-head">
                            <div>
                                <div class="modal-title">"🩹 错题重做"</div>
                                <div class="modal-sub">
                                    {move || {
                                        let list = state.redo_list.get();
                                        let i = state.redo_idx.get().min(list.len().saturating_sub(1));
                                        let Some(item) = list.get(i) else { return String::new() };
                                        let courses = state.courses.get_untracked();
                                        let map = episode_map(&courses);
                                        let subject = map.get(&item.question.source_item_id)
                                            .and_then(|&(c, _)| courses.get(c))
                                            .map(|co| co.subject.clone())
                                            .unwrap_or_default();
                                        let src = src_label(&courses, &map, item.question.source_item_id);
                                        let mut s = String::new();
                                        if list.len() > 1 {
                                            s.push_str(&format!("第 {} / {} 题 · ", i + 1, list.len()));
                                        }
                                        if !subject.is_empty() {
                                            s.push_str(&format!("{} · ", subject));
                                        }
                                        s.push_str(&format!("{} · 错 {} 次", src, item.wrong.times));
                                        s
                                    }}
                                </div>
                            </div>
                            <button class="modal-close" on:click=move |_| redo_exit(state)>"✕"</button>
                        </div>
                        <div class="modal-body">
                            {move || {
                                let list = state.redo_list.get();
                                let idx = state.redo_idx.get();
                                if idx >= list.len() {
                                    let right = list.iter()
                                        .enumerate()
                                        .filter(|(i, _)| {
                                            state.redo_state.get_untracked().get(*i)
                                                .is_some_and(|s| s.mastered)
                                        })
                                        .count();
                                    let mut sub = format!("共 {} 题：答对 {} 题，答错 {} 题", list.len(), right, list.len() - right);
                                    if right > 0 {
                                        sub.push_str(&format!("，其中 {} 题已移出错题本", right));
                                    }
                                    (view! {
                                        <div style="text-align:center;padding:26px 0 30px;">
                                            <div style="font-size:38px;">"🎯"</div>
                                            <div style="font-family:var(--serif);font-size:20px;font-weight:700;margin:8px 0 4px;">"重做完成"</div>
                                            <div style="font-size:13px;color:var(--muted);">{sub}</div>
                                        </div>
                                    }).into_any()
                                } else {
                                    let Some(item) = list.get(idx).cloned() else {
                                        return view! { <div></div> }.into_any();
                                    };
                                    let q = item.question;
                                    let is_multi = q.qtype == QuestionType::Multi;
                                    let picked = move || state.redo_state.get().get(idx).and_then(|s| s.picked.clone());
                                    let out = move || state.redo_state.get().get(idx).and_then(|s| s.outcome.clone());
                                    let courses = state.courses.get_untracked();
                                    let map = episode_map(&courses);
                                    let src = src_label(&courses, &map, q.source_item_id);
                                    let q2 = q.clone();
                                    let options = display_options(&q)
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, text)| {
                                            (view! {
                                                <div class=move || option_class(&picked(), &out(), i, is_multi)
                                                    on:click=move |_| redo_toggle(state, i)>
                                                    <div class="opt-label">{opt_label(i)}</div>
                                                    <span>{text}</span>
                                                </div>
                                            }).into_any()
                                        })
                                        .collect::<Vec<_>>();
                                    let ok_color = move || {
                                        if out().as_ref().map(|o| o.is_correct).unwrap_or(true) {
                                            "var(--green)"
                                        } else {
                                            "var(--red)"
                                        }
                                    };
                                    let exp_label = move || {
                                        if out().as_ref().map(|o| o.is_correct).unwrap_or(true) {
                                            "✓ 答对了 · 解析"
                                        } else {
                                            "✕ 又答错了 · 解析"
                                        }
                                    };
                                    let exp_text = move || {
                                        out().and_then(|o| o.explanation).unwrap_or_else(|| "暂无解析".to_string())
                                    };
                                    (view! {
                                        <div class="quiz-tag-row" style="margin-bottom:10px;">
                                            <span class="quiz-tag" style="background:var(--red-light);color:var(--red);">"错题重做"</span>
                                            <span class="quiz-tag subject">{move || {
                                                let courses = state.courses.get_untracked();
                                                episode_map(&courses).get(&q2.source_item_id)
                                                    .and_then(|&(c, _)| courses.get(c))
                                                    .map(|co| co.subject.clone())
                                                    .unwrap_or_default()
                                            }}</span>
                                            <span class="quiz-tag type">{type_label(&q2)}</span>
                                        </div>
                                        <div class="quiz-question">{q.stem.clone()}</div>
                                        <div class="quiz-options">{options}</div>
                                        <div class="quiz-explanation" class:show=move || out().is_some()
                                            style:border-left-color=ok_color>
                                            <div class="exp-label" style:color=ok_color>{exp_label}</div>
                                            {exp_text}
                                            <div class="exp-src">{format!("出自笔记：{}", src)}</div>
                                        </div>
                                    }).into_any()
                                }
                            }}
                        </div>
                        <div class="modal-foot">
                            {move || {
                                let list = state.redo_list.get();
                                let idx = state.redo_idx.get();
                                if idx >= list.len() {
                                    (view! {
                                        <button class="btn btn-primary" on:click=move |_| redo_exit(state)>"好的"</button>
                                    }).into_any()
                                } else {
                                    let answered = state.redo_state.get().get(idx).is_some_and(|s| s.outcome.is_some());
                                    let mastered = state.redo_state.get().get(idx).is_some_and(|s| s.mastered);
                                    if answered {
                                        (view! {
                                            <span style="margin-right:auto;font-size:13px;font-weight:700;"
                                                style:color=move || if mastered { "var(--green)" } else { "var(--red)" }>
                                                {if mastered { "✓ 已掌握" } else { "✕ 未掌握" }}
                                            </span>
                                            <button class="btn btn-ghost" on:click=move |_| redo_exit(state)>"退出重做"</button>
                                            <button class="btn btn-primary" on:click=move |_| redo_next(state)>
                                                {if state.redo_idx.get_untracked() >= list.len() - 1 { "完成" } else { "下一题 →" }}
                                            </button>
                                        }).into_any()
                                    } else {
                                        (view! {
                                            <span style="margin-right:auto;font-size:12.5px;color:var(--muted-light);">
                                                "选择答案后查看解析与掌握判定"
                                            </span>
                                            <button class="btn btn-primary"
                                                style:display=move || {
                                                    let q = state.redo_list.get_untracked().get(idx).map(|it| it.question.qtype).unwrap_or(QuestionType::Single);
                                                    if q == QuestionType::Multi {
                                                        let pending = state.redo_state.get_untracked().get(idx)
                                                            .is_some_and(|s| s.outcome.is_none() && s.picked.is_some());
                                                        if pending { "inline-block" } else { "none" }
                                                    } else {
                                                        "none"
                                                    }
                                                }
                                                on:click=move |_| redo_confirm(state)>"确认答案"</button>
                                        }).into_any()
                                    }
                                }
                            }}
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 模考结果 ----
    let mock_result_modal = move || {
        (view! {
            <Show when=move || state.mock_result.get().is_some()>
                <div class="modal-overlay" class:show=move || state.mock_result.get().is_some()>
                    <div class="modal" style="width:470px;">
                        <div class="modal-head">
                            <div class="modal-title">"模考结果"</div>
                            <button class="modal-close" on:click=move |_| state.mock_result.set(None)>"✕"</button>
                        </div>
                        <div class="modal-body" style="text-align:center;">
                            <div class="result-score">
                                <span class="res-score">{move || state.mock_result.get().map(|r| r.score).unwrap_or(0)}</span>
                                <span class="res-unit">"分"</span>
                            </div>
                            <div class="result-sub">
                                {move || {
                                    let Some(r) = state.mock_result.get() else { return String::new() };
                                    let pct = if r.total > 0 {
                                        (r.correct as f64 / r.total as f64 * 100.0).round() as u32
                                    } else {
                                        0
                                    };
                                    format!("正确率 {}% · 用时 {}", pct, fmt_duration(r.duration_secs))
                                }}
                            </div>
                            <div class="result-grid">
                                <div class="result-item">
                                    <div class="res-num" style="color:var(--green);">{move || state.mock_result.get().map(|r| r.correct).unwrap_or(0)}</div>
                                    <div class="res-lbl">"答对"</div>
                                </div>
                                <div class="result-item">
                                    <div class="res-num" style="color:var(--red);">{move || state.mock_result.get().map(|r| r.wrong_count).unwrap_or(0)}</div>
                                    <div class="res-lbl">"答错"</div>
                                </div>
                                <div class="result-item">
                                    <div class="res-num" style="color:var(--muted);">{move || state.mock_result.get().map(|r| r.skip_count).unwrap_or(0)}</div>
                                    <div class="res-lbl">"未答"</div>
                                </div>
                            </div>
                            <div class="result-note">"错题已自动加入错题本，可在「错题本」中重做"</div>
                        </div>
                        <div class="modal-foot" style="justify-content:center;">
                            <button class="btn btn-ghost"
                                on:click=move |_| {
                                    state.mock_result.set(None);
                                    switch_view(state, View::Assembly);
                                }>
                                "返回组卷"
                            </button>
                            <button class="btn btn-primary"
                                on:click=move |_| {
                                    state.mock_result.set(None);
                                    switch_view(state, View::Wrong);
                                }>
                                "查看错题本"
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 考试目标设置 ----
    let setup_modal = move || {
        let open = state.setup_open.get();
        (view! {
            <Show when=move || open>
                <div class="modal-overlay" class:show=move || open>
                    <div class="modal" style="width:460px;">
                        <div class="modal-head">
                            <div>
                                <div class="modal-title">"🎯 考试目标设置"</div>
                                <div class="modal-sub">"修改考试目标与考试日期，倒计时同步更新"</div>
                            </div>
                            <button class="modal-close" on:click=move |_| state.setup_open.set(false)>"✕"</button>
                        </div>
                        <div class="ws-switch">
                            <div class="ws-switch-title">"切换备考空间"</div>
                            {ws_rows}
                            <div class="ws-hint">"点击其他空间即可切换 · 学习数据按空间相互独立"</div>
                        </div>
                        <div class="modal-body">
                            <div class="form-row">
                                <label class="form-label">"考试目标（手写填写）"</label>
                                <input class="form-input" placeholder="如：软考 · 系统架构设计师"
                                    prop:value=move || setup_goal.get()
                                    on:input=move |ev| setup_goal.set(event_target_value(&ev)) />
                            </div>
                            <div class="form-row">
                                <label class="form-label">"考试日期"</label>
                                <input class="form-input" type="date"
                                    prop:value=move || setup_date.get()
                                    on:input=move |ev| setup_date.set(event_target_value(&ev)) />
                            </div>
                        </div>
                        <div class="modal-foot">
                            <button class="btn btn-ghost" on:click=move |_| state.setup_open.set(false)>"取消"</button>
                            <button class="btn btn-primary" on:click=save_setup>"保存设置"</button>
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- Agent 接入 ----
    let agent_modal = move || {
        let open = state.agent_open.get();
        (view! {
            <Show when=move || open>
                <div class="modal-overlay" class:show=move || open>
                    <div class="modal" style="width:600px;">
                        <div class="modal-head">
                            <div>
                                <div class="modal-title">"🔌 接入 AI Agent"</div>
                                <div class="modal-sub">"复制下方接入凭证，发送给任意 Agent（如 TRAE），它会自动获得与本系统交互的能力"</div>
                            </div>
                            <button class="modal-close" on:click=move |_| state.agent_open.set(false)>"✕"</button>
                        </div>
                        <div class="modal-body">
                            <div class="agent-steps">
                                <div class="agent-step"><span class="step-no">1</span><span><b>"复制接入凭证"</b>"　点击下方「复制接入凭证」按钮"</span></div>
                                <div class="agent-step"><span class="step-no">2</span><span><b>"发送给任意 Agent"</b>"　把凭证粘贴到 Agent 对话中（TRAE 等均可）"</span></div>
                                <div class="agent-step"><span class="step-no">3</span><span><b>"自动装配能力"</b>"　Agent 访问我们的服务，服务返回对应的 Skill、提示词与 MCP 配置"</span></div>
                                <div class="agent-step"><span class="step-no">4</span><span><b>"开始协作"</b>"　Agent 为你生成目录 / 笔记 / 批注 / 习题，读取错题记录，发起复盘"</span></div>
                            </div>
                            <div class="agent-config">{move || state.agent_text.get()}</div>
                            <div class="agent-note">"🔒 凭证与当前登录用户绑定，Agent 只能读写你本人的学习数据，不同用户之间严格隔离，不会串数据。"</div>
                        </div>
                        <div class="modal-foot">
                            <button class="btn btn-ghost"
                                on:click=move |_| {
                                    state.agent_open.set(false);
                                    state.toast("随时可从左下角头像菜单 → 🔌 Agent 接入凭证 重新打开");
                                }>
                                "稍后接入"
                            </button>
                            <button class="btn btn-primary"
                                on:click=move |_| copy_credential(state, state.agent_text.get_untracked())>
                                "📋 复制接入凭证"
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 批注编辑 ----
    let anno_modal = move || {
        let open = state.anno_open.get();
        (view! {
            <Show when=move || open>
                <div class="modal-overlay" class:show=move || open>
                    <div class="modal" style="width:480px;">
                        <div class="modal-head">
                            <div>
                                <div class="modal-title">"✏️ 补充批注"</div>
                                <div class="modal-sub">"为选中内容添加个人笔记或疑问"</div>
                            </div>
                            <button class="modal-close" on:click=move |_| state.anno_open.set(false)>"✕"</button>
                        </div>
                        <div class="modal-body">
                            <div style="font-size:12px;color:var(--muted);margin-bottom:6px;">"选中原文"</div>
                            <div style="background:var(--surface-2);border-left:3px solid var(--gold);padding:9px 14px;border-radius:0 8px 8px 0;font-size:13.5px;margin-bottom:14px;line-height:1.7;">
                                {move || state.anno_quote.get()}
                            </div>
                            <textarea style="width:100%;min-height:110px;padding:12px;border:1px solid var(--border);border-radius:var(--radius-sm);font-family:var(--sans);font-size:13.5px;resize:vertical;outline:none;background:var(--surface);color:var(--ink);"
                                placeholder="输入批注内容…"
                                prop:value=move || state.anno_text.get()
                                on:input=move |ev| state.anno_text.set(event_target_value(&ev))></textarea>
                        </div>
                        <div class="modal-foot">
                            <button class="btn btn-ghost" on:click=move |_| state.anno_open.set(false)>"取消"</button>
                            <button class="btn btn-primary"
                                on:click=move |_| {
                                    let text = state.anno_text.get_untracked().trim().to_string();
                                    if text.is_empty() {
                                        state.toast("批注内容不能为空");
                                        return;
                                    }
                                    if let Some(save) = state.anno_save.get_untracked() {
                                        save(text);
                                    }
                                    state.anno_text.set(String::new());
                                    state.anno_open.set(false);
                                }>
                                "保存批注"
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 批注详情 ----
    let anno_detail_modal = move || {
        (view! {
            <Show when=move || state.anno_detail.get().is_some()>
                <div class="modal-overlay" class:show=move || state.anno_detail.get().is_some()>
                    <div class="modal" style="width:460px;">
                        <div class="modal-head">
                            <div class="modal-title">"📌 批注详情"</div>
                            <button class="modal-close" on:click=move |_| state.anno_detail.set(None)>"✕"</button>
                        </div>
                        <div class="modal-body">
                            <div style="font-size:12px;color:var(--muted);margin-bottom:6px;">"批注原文"</div>
                            <div style="background:var(--surface-2);border-left:3px solid var(--gold);padding:9px 14px;border-radius:0 8px 8px 0;font-size:13.5px;margin-bottom:14px;line-height:1.7;">
                                {move || state.anno_detail.get().map(|d| d.quote).unwrap_or_default()}
                            </div>
                            <div style="font-size:12px;color:var(--muted);margin-bottom:6px;">"批注内容"</div>
                            <span class="badge"
                                style:background=move || if state.anno_detail.get().map(|d| d.mine).unwrap_or(false) { "var(--accent-light)" } else { "var(--gold-light)" }
                                style:color=move || if state.anno_detail.get().map(|d| d.mine).unwrap_or(false) { "var(--accent-deep)" } else { "var(--gold)" }>
                                {move || if state.anno_detail.get().map(|d| d.mine).unwrap_or(false) { "我的批注" } else { "AI 批注（老师强调 / 考点）" }}
                            </span>
                            <div style="margin-top:10px;font-size:13.5px;line-height:1.85;color:var(--ink);">
                                {move || state.anno_detail.get().map(|d| d.text).unwrap_or_default()}
                            </div>
                        </div>
                        <div class="modal-foot">
                            {move || {
                                let Some(d) = state.anno_detail.get() else {
                                    return (view! { <div></div> }).into_any();
                                };
                                if d.mine {
                                    let anno_id = d.anno_id;
                                    (view! {
                                        <button class="btn btn-ghost"
                                            on:click=move |_| {
                                                let st = state;
                                                spawn_local(async move {
                                                    match api::delete_annotation(anno_id).await {
                                                        Ok(()) => {
                                                            st.toast("批注已删除");
                                                            if let Some(f) = st.note_reload.get_untracked() {
                                                                f();
                                                            }
                                                            st.anno_detail.set(None);
                                                        }
                                                        Err(e) => st.toast(&format!("删除失败：{}", e)),
                                                    }
                                                });
                                            }>
                                            "🗑 删除批注"
                                        </button>
                                        <button class="btn btn-primary" on:click=move |_| state.anno_detail.set(None)>"知道了"</button>
                                    }).into_any()
                                } else {
                                    (view! {
                                        <button class="btn btn-ghost"
                                            on:click=move |_| state.toast("已发送给 Agent 请求解答（演示）")>
                                            "🤖 让 AI 解释"
                                        </button>
                                        <button class="btn btn-primary" on:click=move |_| state.anno_detail.set(None)>"知道了"</button>
                                    }).into_any()
                                }
                            }}
                        </div>
                    </div>
                </div>
            </Show>
        }).into_any()
    };

    // ---- 批注悬浮按钮（选中文本后出现，位置随鼠标） ----
    let fab = move || {
        let pos = state.fab_pos.get();
        (view! {
            <Show when=move || pos.is_some()>
                <button class="anno-fab" id="annoFab"
                    style:left=move || format!("{}px", state.fab_pos.get().map(|(x, _)| x).unwrap_or(0.0))
                    style:top=move || format!("{}px", state.fab_pos.get().map(|(_, y)| y).unwrap_or(0.0))
                    style:display="block"
                    on:click=move |_| annotate_from_fab(state)>
                    "✏️ 批注"
                </button>
            </Show>
        }).into_any()
    };

    view! {
        <>
            {confirm_modal}
            {preview_modal}
            {redo_modal}
            {mock_result_modal}
            {setup_modal}
            {agent_modal}
            {anno_modal}
            {anno_detail_modal}
            {fab}
            <div class="toast" class:show=toast_show>{toast_msg}</div>
        </>
    }
}
