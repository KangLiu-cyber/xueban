//! 应用外壳：初始化数据加载 / 全局事件 / Sidebar / Topbar / 视图切换与常驻视图。

use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use leptos::task::spawn_local;

use crate::api::{self, DrawQuery, ItemKind, ItemNode};
use crate::state::{self, episode_map, AppState, View};
use crate::views::ui::{close_all_dd, fire_view_switch_hooks};
use crate::views::wrong::refresh_wrong;

/// 切换主视图：触发视图钩子、清 topbar 覆盖、切视图、回顶。
pub fn switch_view(state: AppState, v: View) {
    fire_view_switch_hooks();
    state.topbar_override.set(None);
    state.view.set(v);
    if let Some(el) = document().get_element_by_id("content-area") {
        el.set_scroll_top(0);
    }
}

fn close_all_modals(state: AppState) {
    state.confirm.set(None);
    state.preview_open.set(false);
    state.anno_detail.set(None);
    state.mock_result.set(None);
    state.setup_open.set(false);
    state.agent_open.set(false);
    state.anno_open.set(false);
    state.redo_open.set(false);
    state.fab_pos.set(None);
}

/// 初始化：空间回退 → 树 / 课程 / 题库 / 错题本。
pub(crate) async fn init_data(state: AppState) {
    let ws = match state.workspace.get_untracked() {
        Some(w) => Some(w),
        None => match api::list_workspaces().await {
            Ok(list) if !list.is_empty() => {
                state.workspaces.set(list.clone());
                let w = list[0].clone();
                state.set_workspace(&w);
                Some(w)
            }
            _ => None,
        },
    };
    let Some(w) = ws else {
        state.setup_open.set(true);
        return;
    };
    let tree = api::tree(w.id).await.unwrap_or_default();
    state.tree.set(tree.clone());
    state.courses.set(state::derive_courses(&tree));
    match api::draw(&DrawQuery {
        workspace_id: w.id,
        scope: None,
        count: Some(100),
    })
    .await
    {
        Ok(pool) => state.pool.set(pool),
        Err(e) => state.toast(&format!("题库加载失败：{}", e)),
    }
    refresh_wrong(state).await;
}

fn topbar_title(state: AppState) -> (String, String) {
    if let Some((t, s)) = state.topbar_override.get() {
        return (t, s);
    }
    match state.view.get() {
        View::Quiz => ("刷题".into(), "按范围抽题，即时判分".into()),
        View::Wrong => ("错题本".into(), "反复重做直到掌握".into()),
        View::Assembly => ("组卷".into(), "挑选题目，一键组卷模考".into()),
        View::Mock => ("模考".into(), "限时答题，自动判分".into()),
        View::Notes => match state.episode.get() {
            Some((c, e)) => {
                let courses = state.courses.get();
                let sub = courses
                    .get(c)
                    .map(|course| format!("{} · {}", course.name, course.subject))
                    .unwrap_or_default();
                (format!("{} · 笔记", state::scope_name(&courses, c, e)), sub)
            }
            None => ("课程与笔记".into(), "选择一集开始学习".into()),
        },
    }
}

fn cd_chip_text(state: AppState) -> String {
    let d = state.workspace.get().and_then(|w| w.exam_date);
    match d {
        Some(d) => format!("⏳ 距离考试 {} 天", state::exam_days_left(Some(d))),
        None => "⏳ 距离考试 -- 天".to_string(),
    }
}

// ---- 目录树 ----

/// 递归渲染目录：集节点 → tree-item；无笔记子节点的目录 → 可折叠 tree-folder。
fn tree_rows(
    state: AppState,
    children: &[ItemNode],
    map: &HashMap<i64, (usize, usize)>,
) -> Vec<AnyView> {
    let mut out = Vec::new();
    for node in children {
        match node.item.kind {
            ItemKind::Dir => {
                let has_notes = node.children.iter().any(|c| c.item.kind == ItemKind::Note);
                if has_notes {
                    out.push(episode_row(state, node, map));
                } else {
                    out.extend(folder_row(state, node, map));
                }
            }
            ItemKind::Note => out.push(episode_row(state, node, map)),
        }
    }
    out
}

fn episode_row(state: AppState, node: &ItemNode, map: &HashMap<i64, (usize, usize)>) -> AnyView {
    let node_id = node.item.id;
    let num = map.get(&node_id).map(|(_, e)| e + 1).unwrap_or(1);
    let title = node.item.name.clone();
    let is_cur = move || match state.episode.get() {
        Some((c, e)) => state.courses.with_untracked(|cs| {
            cs.get(c)
                .and_then(|co| co.episodes.get(e))
                .map(|ep| ep.node_id == node_id)
                .unwrap_or(false)
        }),
        None => false,
    };
    let ce_opt = map.get(&node_id).copied();
    let go = move |_| {
        if let Some(ce) = ce_opt {
            state.episode.set(Some(ce));
            switch_view(state, View::Notes);
        }
    };
    (view! {
        <div class="tree-item" class:active=is_cur on:click=go>
            <span class="tree-icon">"📝"</span>
            {format!("第{}集 {}", num, title)}
        </div>
    },)
        .into_any()
}

fn folder_row(
    state: AppState,
    node: &ItemNode,
    map: &HashMap<i64, (usize, usize)>,
) -> Vec<AnyView> {
    let open = RwSignal::new(true);
    let count = state::count_episodes(node);
    let name = node.item.name.clone();
    let children_html = tree_rows(state, &node.children, map);
    let toggle = move |_| open.set(!open.get_untracked());
    vec![
        (
            view! {
                <div class="tree-folder" class:collapsed=move || !open.get() on:click=toggle>
                    <span class="folder-caret">"▼"</span>
                    "📂 " {name}
                    <span class="folder-count">{format!("{} 集", count)}</span>
                </div>
            },
        ).into_any(),
        (
            view! {
                <div class="tree-children" style:display=move || if open.get() { "block" } else { "none" }>
                    {children_html}
                </div>
            },
        ).into_any(),
    ]
}

// ---- Sidebar ----

#[component]
fn Sidebar(state: AppState, user_menu: RwSignal<bool>) -> impl IntoView {
    let user = move || state.user.get();
    let avatar_char = move || {
        user()
            .and_then(|u| u.nickname.or(Some(u.account)))
            .map(|s| s.chars().next().unwrap_or('学').to_string())
            .unwrap_or_else(|| "学".to_string())
    };
    let user_name = move || {
        user()
            .map(|u| u.nickname.unwrap_or(u.account))
            .unwrap_or_default()
    };
    let ws_name = move || {
        state
            .workspace
            .get()
            .map(|w| w.name)
            .unwrap_or_else(|| "备考空间".to_string())
    };
    let cd_days = move || {
        state
            .workspace
            .get()
            .and_then(|w| w.exam_date)
            .map(|d| state::exam_days_left(Some(d)).to_string())
            .unwrap_or_else(|| "--".to_string())
    };
    let cd_date = move || {
        state.workspace.get().map(|w| {
            let d = w.exam_date.map(|d| d.to_string()).unwrap_or_default();
            if d.is_empty() {
                w.name
            } else {
                format!("{} · {}", d, w.name)
            }
        })
    };
    let quiz_count = move || format!("{} 题", state.pool.get().len());
    let wrong_count = move || state.wrong_badge.get();
    let tree_html = move || {
        // P1-2：订阅 courses / tree，目录加载与集数变化后自动重渲染
        let courses = state.courses.get();
        let map = episode_map(&courses);
        let tree = state.tree.get();
        tree_rows(state, &tree, &map)
    };
    let do_logout = move |_| {
        let st = state;
        spawn_local(async move {
            let _ = api::logout().await;
            st.clear_auth();
            st.toast("已退出登录");
        });
    };

    view! {
        <div class="sidebar-header">
            <div class="workspace-selector" on:click=move |_| state.setup_open.set(true)>
                <span class="ws-name">{ws_name}</span>
                <span class="ws-badge">备考中</span>
            </div>
            <div class="exam-countdown" on:click=move |_| state.setup_open.set(true)>
                <div class="cd-num"><span>{cd_days}</span><span class="cd-unit">天</span></div>
                <div>
                    <div class="cd-label">距离考试</div>
                    <div class="cd-date">{cd_date}</div>
                </div>
            </div>
        </div>
        <div class="sidebar-nav">
            <div class="nav-section">
                <div class="nav-section-title">练习</div>
                <div class="nav-item" class:active=move || state.view.get() == View::Quiz
                    on:click=move |_| switch_view(state, View::Quiz)>
                    <span class="icon">"✏️"</span>
                    " 刷题 "
                    <span class="count">{quiz_count}</span>
                </div>
                <div class="nav-item" class:active=move || state.view.get() == View::Wrong
                    on:click=move |_| switch_view(state, View::Wrong)>
                    <span class="icon">"🩹"</span>
                    " 错题本 "
                    <span class="count">{wrong_count}</span>
                </div>
                <div class="nav-item" class:active=move || state.view.get() == View::Assembly
                    on:click=move |_| switch_view(state, View::Assembly)>
                    <span class="icon">"📜"</span>
                    " 组卷"
                </div>
            </div>
            <div class="nav-section">
                <div class="nav-section-title">"内容 · AI 生成（点目录可折叠）"</div>
                {tree_html}
            </div>
        </div>
        <div class="sidebar-footer" on:click=move |_| user_menu.set(!user_menu.get_untracked())>
            <div class="avatar">{avatar_char}</div>
            <div>
                <div class="user-name">{user_name}</div>
                <div class="user-plan">"账号 · 接入设置 ⌄"</div>
            </div>
            <div class="user-menu" class:show=move || user_menu.get()>
                <div class="user-menu-item" on:click=move |_| { user_menu.set(false); state.agent_open.set(true); }>
                    "🔌 Agent 接入凭证"
                </div>
                <div class="user-menu-item" on:click=move |_| { user_menu.set(false); state.setup_open.set(true); }>
                    "🎯 考试目标设置"
                </div>
                <div class="user-menu-item danger" on:click=do_logout>
                    "👋 退出登录"
                </div>
            </div>
        </div>
    }
}

// ---- Topbar ----

#[component]
fn Topbar(state: AppState) -> impl IntoView {
    let title = move || topbar_title(state).0;
    let sub = move || topbar_title(state).1;
    view! {
        <div class="topbar">
            <div>
                <div class="topbar-title">{title}</div>
                <div class="topbar-sub">{sub}</div>
            </div>
            <div class="topbar-spacer"></div>
            <div class="cd-chip">{move || cd_chip_text(state)}</div>
            <button class="btn btn-ghost btn-sm" on:click=move |_| { let _ = window().print(); }>
                "🖨 打印"
            </button>
            <button class="btn btn-ghost btn-sm" on:click=move |_| state.toast("全局搜索：笔记 / 题目 / 错题")>
                "🔍 搜索"
            </button>
        </div>
    }
}

// ---- Shell ----

#[component]
pub fn Shell() -> impl IntoView {
    let state = use_context::<AppState>().expect("AppState missing");
    let user_menu = RwSignal::new(false);

    Effect::new(move |_| {
        let win = window();
        let click = Closure::<dyn Fn(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let target = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok());
            let hit = |sel: &str| {
                target
                    .as_ref()
                    .and_then(|el| el.closest(sel).ok().flatten())
                    .is_some()
            };
            if !hit(".dd") {
                close_all_dd();
            }
            if !hit(".sidebar-footer") {
                user_menu.set(false);
            }
        });
        win.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())
            .expect("add click listener");
        click.forget();

        let keydown = Closure::<dyn Fn(web_sys::Event)>::new(move |ev: web_sys::Event| {
            if let Ok(k) = ev.clone().dyn_into::<web_sys::KeyboardEvent>() {
                if k.key() == "Escape" {
                    close_all_dd();
                    user_menu.set(false);
                    close_all_modals(state);
                }
            }
        });
        win.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())
            .expect("add keydown listener");
        keydown.forget();

        spawn_local(async move {
            init_data(state).await;
        });
    });

    view! {
        <>
            <div class="app">
                <div class="sidebar">
                    <Sidebar state=state user_menu=user_menu />
                </div>
                <div class="main">
                    <Topbar state=state />
                    <div class="content-area" id="content-area">
                        <div class="view" id="view-notes" class:active=move || state.view.get() == View::Notes>
                            <crate::views::notes::NotesView state=state />
                        </div>
                        <div class="view" id="view-quiz" class:active=move || state.view.get() == View::Quiz>
                            <crate::views::quiz::QuizView state=state />
                        </div>
                        <div class="view" id="view-wrong" class:active=move || state.view.get() == View::Wrong>
                            <crate::views::wrong::WrongView state=state />
                        </div>
                        <div class="view" id="view-assembly" class:active=move || state.view.get() == View::Assembly>
                            <crate::views::assembly::AssemblyView state=state />
                        </div>
                        <div class="view" id="view-mock" class:active=move || state.view.get() == View::Mock>
                            <crate::views::mock::MockView state=state />
                        </div>
                    </div>
                </div>
            </div>
            <crate::views::modals::Modals state=state />
        </>
    }
}
