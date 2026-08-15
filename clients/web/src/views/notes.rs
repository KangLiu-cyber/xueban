//! 笔记视图：集切换条（搜索下拉 / 进度）、笔记卡片（Markdown 渲染 +
//! 批注高亮）、文本选择批注交互（浮动批注按钮 / 弹窗 / 详情）。

use std::cell::RefCell;
use std::sync::Arc;

use js_sys::JsString;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use web_sys::{Element, Event, MouseEvent, Selection};

use crate::api::{self, AnnotationInput, DrawQuery, ItemBundle};
use crate::state::{fmt_date_ymd, AnnoDetail, AppState, QuizScope, View};
use crate::views::shell::switch_view;
use crate::views::ui::register_dd_closer;

// 本次选区（item_id, 引用文本）：从 check_selection 记录，供批注弹窗使用。
thread_local! {
    static SAVED_ANNO: RefCell<Option<(i64, String)>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoteTab {
    Note,
    Quiz,
}

// ---- 选区与批注 ----

fn node_el(n: &web_sys::Node) -> Option<Element> {
    if n.node_type() == 1 {
        n.clone().dyn_into::<Element>().ok()
    } else {
        n.parent_element()
    }
}

/// Selection/Range 无 toString 绑定，经 Reflect 调用原生 toString。
fn sel_text(sel: &Selection) -> Option<String> {
    let v = js_sys::Reflect::get(sel.as_ref(), &JsString::from("toString")).ok()?;
    let f: js_sys::Function = v.dyn_into().ok()?;
    let s: JsString = f.call0(sel.as_ref()).ok()?.dyn_into().ok()?;
    s.as_string()
}

/// 检查选区是否落在笔记正文内：≥2 字、起点终点同段，展示浮动批注按钮。
fn check_selection(state: AppState) {
    let hide = || state.fab_pos.set(None);
    let Ok(Some(sel)) = window().get_selection() else {
        hide();
        return;
    };
    if sel.is_collapsed() || sel.range_count() == 0 {
        hide();
        return;
    }
    let Some(a) = sel.anchor_node().and_then(|n| node_el(&n)) else {
        hide();
        return;
    };
    let Some(f) = sel.focus_node().and_then(|n| node_el(&n)) else {
        hide();
        return;
    };
    let Ok(Some(body_a)) = a.closest(".note-body") else {
        hide();
        return;
    };
    let Ok(Some(body_f)) = f.closest(".note-body") else {
        hide();
        return;
    };
    if body_a.id() != body_f.id() {
        hide();
        return;
    }
    let Some(text) = sel_text(&sel) else {
        hide();
        return;
    };
    let quote = text.trim();
    if quote.len() < 2 {
        hide();
        return;
    }
    let Ok(Some(card)) = body_a.closest(".note-card") else {
        hide();
        return;
    };
    let Some(item_id) = card
        .get_attribute("data-item-id")
        .and_then(|s| s.parse::<i64>().ok())
    else {
        hide();
        return;
    };
    let Ok(range) = sel.get_range_at(0) else {
        hide();
        return;
    };
    let rect = range.get_bounding_client_rect();
    let inner_w: f64 = window()
        .inner_width()
        .ok()
        .and_then(|w| w.as_f64())
        .unwrap_or(0.0);
    let left = (rect.left() + rect.width() / 2.0).clamp(70.0, (inner_w - 70.0).max(70.0));
    let top = (rect.top() - 44.0).max(52.0);
    SAVED_ANNO.with(|s| *s.borrow_mut() = Some((item_id, quote.to_string())));
    state.fab_pos.set(Some((left, top)));
}

fn open_anno_modal(state: AppState) {
    state.fab_pos.set(None);
    let Some((item_id, quote)) = SAVED_ANNO.with(|s| s.borrow().clone()) else {
        state.toast("请先在正文中选中要批注的文字");
        return;
    };
    state.anno_quote.set(format!("「{}」", quote));
    state.anno_text.set(String::new());
    let st = state;
    state.anno_save.set(Some(Arc::new(move |text: String| {
        let quote = quote.clone();
        spawn_local(async move {
            match api::add_annotation(
                item_id,
                &AnnotationInput {
                    anchor: quote,
                    text,
                },
            )
            .await
            {
                Ok(_) => {
                    st.toast("批注已保存 · 点击批注可随时查看详情");
                    if let Some(reload) = st.note_reload.get_untracked() {
                        reload();
                    }
                }
                Err(e) => st.toast(&format!("批注保存失败：{}", e)),
            }
        });
    })));
    state.anno_open.set(true);
}

/// 浮动批注按钮点击（fab 在 modals.rs 渲染）。
pub fn annotate_from_fab(state: AppState) {
    open_anno_modal(state);
}

fn annotate_from_button(state: AppState) {
    open_anno_modal(state);
}

// ---- 集切换 ----

/// 全部课程下的扁平集列表，跨课程前后翻集。
fn flat_episodes(state: AppState) -> Vec<(usize, usize)> {
    state
        .courses
        .get_untracked()
        .iter()
        .enumerate()
        .flat_map(|(c, co)| co.episodes.iter().enumerate().map(move |(e, _)| (c, e)))
        .collect()
}

fn switch_episode(state: AppState, delta: i32) {
    let flat = flat_episodes(state);
    if flat.is_empty() {
        return;
    }
    let cur = state
        .episode
        .get_untracked()
        .and_then(|x| flat.iter().position(|&y| y == x))
        .unwrap_or(0);
    let nxt = cur as i32 + delta;
    if nxt < 0 || nxt >= flat.len() as i32 {
        state.toast(if delta < 0 {
            "已经是第一集了"
        } else {
            "已经是最后一集了"
        });
        return;
    }
    state.episode.set(Some(flat[nxt as usize]));
    switch_view(state, View::Notes);
}

/// 加载当前集全部笔记 bundle，标记已学并注册重载回调。
async fn load_episode(state: AppState, quiz_count: RwSignal<u32>, c: usize, e: usize) {
    let Some(course) = state.courses.get_untracked().get(c).cloned() else {
        return;
    };
    let Some(ep) = course.episodes.get(e).cloned() else {
        return;
    };
    let mut bundles = Vec::new();
    for note in &ep.notes {
        if let Ok(b) = api::item_bundle(note.id).await {
            bundles.push(b);
        }
    }
    state.note_bundles.set(bundles);
    state.mark_learned(ep.node_id);
    if let Some(ws) = state.workspace.get_untracked() {
        if let Ok(qs) = api::draw(&DrawQuery {
            workspace_id: ws.id,
            scope: Some(ep.node_id),
            count: Some(100),
        })
        .await
        {
            quiz_count.set(qs.len() as u32);
        }
    }
    let st = state;
    state.note_reload.set(Some(Arc::new(move || {
        let st = st;
        spawn_local(async move {
            load_episode(st, quiz_count, c, e).await;
        });
    })));
}

// ---- 渲染 ----

fn note_card(state: AppState, b: &ItemBundle) -> AnyView {
    let subject = match state.episode.get_untracked() {
        Some((c, _)) => state
            .courses
            .get_untracked()
            .get(c)
            .map(|co| co.subject.clone())
            .unwrap_or_default(),
        None => String::new(),
    };
    let md = crate::markdown::render_markdown(b.item.content.as_deref().unwrap_or(""));
    let html = crate::markdown::html_with_annotations(&md, &b.annotations);
    let mins = ((b.item.content.as_deref().unwrap_or("").len() as u32) / 1000).max(1);
    let item_id = b.item.id;
    let title = b.item.name.clone();
    let date = fmt_date_ymd(b.item.created_at);
    (
        view! {
            <div class="note-card" data-item-id={item_id}>
                <div class="note-header">
                    <div class="note-meta">
                        <span class="tag">{subject}</span>
                        <span class="tag" style="background:var(--green-light);color:var(--green);">"AI 生成"</span>
                        <span>{date}</span>
                        <span>{format!("时长 {} 分钟", mins)}</span>
                        <span>"来源：B站视频 → AI 生成笔记与批注"</span>
                    </div>
                    <h1 class="note-title">{title}</h1>
                </div>
                <div class="note-section">
                    <div class="note-body" inner_html={html}></div>
                </div>
            </div>
        },
    ).into_any()
}

fn ep_dd_rows(state: AppState, cur: Option<(usize, usize)>, filter: &str) -> Vec<AnyView> {
    let f = filter.trim().to_lowercase();
    let courses = state.courses.get_untracked();
    let mut rows: Vec<AnyView> = Vec::new();
    let mut last_course: Option<i64> = None;
    for (c, co) in courses.iter().enumerate() {
        for (e, ep) in co.episodes.iter().enumerate() {
            let name = format!("第{}集 {}", e + 1, ep.title);
            if !f.is_empty() {
                let hay = format!("{} {}", name, co.name).to_lowercase();
                if !hay.contains(&f) {
                    continue;
                }
            }
            if last_course != Some(co.dir_id) {
                rows.push((view! { <div class="dd-group">{co.name.clone()}</div> },).into_any());
                last_course = Some(co.dir_id);
            }
            let is_cur = cur == Some((c, e));
            let done = state.learned.get_untracked().contains(&ep.node_id);
            rows.push(
                (view! {
                    <div class="dd-item" class:active=is_cur
                        on:click=move |_| {
                            state.episode.set(Some((c, e)));
                            switch_view(state, View::Notes);
                        }>
                        <span>{name.clone()}</span>
                        <span class="di-meta" class:done=done>
                            {if done { "已学 ✓" } else { "未学" }}
                        </span>
                    </div>
                },)
                    .into_any(),
            );
        }
    }
    if rows.is_empty() {
        rows.push((
            view! {
                <div style="padding:14px 10px;font-size:12px;color:var(--muted-light)">"没有匹配的集数"</div>
            },
        ).into_any());
    }
    rows
}

#[component]
pub fn NotesView(state: AppState) -> impl IntoView {
    let tab = RwSignal::new(NoteTab::Note);
    let ep_open = RwSignal::new(false);
    let ep_search = RwSignal::new(String::new());
    let quiz_count = RwSignal::new(0u32);

    register_dd_closer(move || ep_open.set(false));

    // 初次有课程但未选集时默认进入第一集。
    Effect::new(move |_| {
        let courses = state.courses.get();
        let ep = state.episode.get();
        if ep.is_none() && !courses.is_empty() {
            state.episode.set(Some((0, 0)));
        }
    });

    // 集切换 → 加载 bundles / 习题数 / 重载回调 / 重置弹窗态。
    Effect::new(move |_| {
        let Some((c, e)) = state.episode.get() else {
            return;
        };
        tab.set(NoteTab::Note);
        ep_open.set(false);
        ep_search.set(String::new());
        SAVED_ANNO.with(|s| *s.borrow_mut() = None);
        state.fab_pos.set(None);
        state.note_reload.set(None);
        spawn_local(async move {
            load_episode(state, quiz_count, c, e).await;
        });
    });

    // 全局监听：正文选区 → 批注按钮；正文点击批注 → 详情。
    Effect::new(move |_| {
        let st = state;
        let up = Closure::<dyn Fn(MouseEvent)>::new(move |ev: MouseEvent| {
            let target = ev.target().and_then(|t| t.dyn_into::<Element>().ok());
            let hit = |sel: &str| {
                target
                    .as_ref()
                    .and_then(|el| el.closest(sel).ok().flatten())
                    .is_some()
            };
            if hit("#annoFab") || hit(".modal-overlay") {
                return;
            }
            let st = st;
            gloo_timers::callback::Timeout::new(10, move || check_selection(st)).forget();
        });
        let doc: web_sys::EventTarget = document().unchecked_into();
        doc.add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())
            .expect("add mouseup listener");
        up.forget();

        let st = state;
        let click = Closure::<dyn Fn(Event)>::new(move |ev: Event| {
            let Some(target) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                return;
            };
            let Ok(Some(anno)) = target.closest(".annotation-highlight") else {
                return;
            };
            let g = |k: &str| anno.get_attribute(k);
            let Some(anno_id) = g("data-anno-id").and_then(|s| s.parse::<i64>().ok()) else {
                return;
            };
            let Some(item_id) = g("data-item-id").and_then(|s| s.parse::<i64>().ok()) else {
                return;
            };
            st.anno_detail.set(Some(AnnoDetail {
                quote: g("data-quote").unwrap_or_default(),
                text: g("data-text").unwrap_or_default(),
                mine: g("data-mine").map(|s| s == "true").unwrap_or(false),
                item_id,
                anno_id,
            }));
        });
        if let Some(area) = document().get_element_by_id("content-area") {
            let tgt: web_sys::EventTarget = area.unchecked_into();
            tgt.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())
                .expect("add content click listener");
            click.forget();
        }

        // 滚动隐藏浮动按钮。
        let st = state;
        let scroll = Closure::<dyn Fn(Event)>::new(move |_ev: Event| {
            st.fab_pos.set(None);
        });
        if let Some(area) = document().get_element_by_id("content-area") {
            let tgt: web_sys::EventTarget = area.unchecked_into();
            tgt.add_event_listener_with_callback("scroll", scroll.as_ref().unchecked_ref())
                .expect("add scroll listener");
            scroll.forget();
        }
    });

    let ep_current = move || match state.episode.get() {
        Some((c, e)) => state.courses.with_untracked(|cs| {
            cs.get(c)
                .and_then(|co| co.episodes.get(e))
                .map(|ep| format!("第{}集 {}", e + 1, ep.title))
                .unwrap_or_default()
        }),
        None => String::new(),
    };
    let ep_progress = move || match state.episode.get() {
        Some((c, _)) => {
            let (learned, total) = state.course_progress(c);
            format!("已学 {} / {} 集", learned, total)
        }
        None => String::new(),
    };
    let quiz_tab_click = move |_| {
        tab.set(NoteTab::Quiz);
        let Some((c, e)) = state.episode.get_untracked() else {
            return;
        };
        let Some(course) = state.courses.get_untracked().get(c).cloned() else {
            return;
        };
        let Some(ep) = course.episodes.get(e) else {
            return;
        };
        state.quiz_scope.set(QuizScope::Episode(c, e));
        state.topbar_override.set(Some((
            format!("第{}集 {} · 本集习题", e + 1, ep.title),
            format!("{} · {}", course.name, course.subject),
        )));
        state.view.set(View::Quiz);
        if let Some(el) = document().get_element_by_id("content-area") {
            el.set_scroll_top(0);
        }
    };
    let dd_rows = move || {
        let cur = state.episode.get_untracked();
        ep_dd_rows(state, cur, &ep_search.get())
    };
    let cards = move || {
        state
            .note_bundles
            .get()
            .iter()
            .map(|b| note_card(state, b))
            .collect::<Vec<_>>()
    };

    view! {
        <div class="notes-view">
            <div class="episode-bar">
                <button class="ep-btn" on:click=move |_| switch_episode(state, -1)>"← 上一集"</button>
                <div class="dd" class:open=move || ep_open.get()>
                    <div class="dd-current" on:click=move |_| ep_open.set(!ep_open.get_untracked())>
                        <span>{ep_current}</span>
                        <span class="dd-caret">"▾"</span>
                    </div>
                    <div class="dd-panel">
                        <input
                            class="dd-search"
                            placeholder="搜索集数或标题…"
                            prop:value=move || ep_search.get()
                            on:input=move |ev| ep_search.set(event_target_value(&ev))
                            on:click=move |ev| ev.stop_propagation()
                        />
                        <div class="dd-list">{dd_rows}</div>
                    </div>
                </div>
                <button class="ep-btn" on:click=move |_| switch_episode(state, 1)>"下一集 →"</button>
                <span class="ep-progress">{ep_progress}</span>
            </div>

            <div class="note-tabs">
                <div class="note-tab" class:active=move || tab.get() == NoteTab::Note
                    on:click=move |_| tab.set(NoteTab::Note)>
                    "📝 笔记"
                </div>
                <div class="note-tab" class:active=move || tab.get() == NoteTab::Quiz on:click=quiz_tab_click>
                    "✏️ 本集习题"
                    <span class="tab-count">{quiz_count}</span>
                </div>
            </div>

            {cards}

            <div class="note-actions">
                <button class="btn btn-primary btn-sm" on:click=move |_| annotate_from_button(state)>
                    "✏️ 补充批注"
                </button>
                <span style="font-size:12px;color:var(--muted-light);align-self:center;">
                    "先在正文中选中文字即可添加批注 · 点击已有批注可查看详情"
                </span>
            </div>
        </div>
    }
}
