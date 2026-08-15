//! 登录页：账号登录 / 注册 → 无备考空间时原地进入「创建备考空间」第二步。
//!
//! 登录成功先 `persist_creds`（写 localStorage，不置 user），查空间：
//! 已有空间 → 置 user 进入系统；为空 → 停留第二步，确认创建后再进入。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, LoginRequest, RegisterRequest, UserDto, WorkspaceInput};
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthTab {
    Login,
    Register,
}

#[component]
pub fn AuthPage() -> impl IntoView {
    let state = use_context::<AppState>().expect("AppState missing");
    let tab = RwSignal::new(AuthTab::Login);
    let account = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let reg_account = RwSignal::new(String::new());
    let reg_password = RwSignal::new(String::new());
    let reg_password2 = RwSignal::new(String::new());
    let goal = RwSignal::new(String::new());
    let date = RwSignal::new("2026-11-07".to_string());
    let step2 = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let pending_user: RwSignal<Option<UserDto>> = RwSignal::new(None);

    let after_auth = move |st: AppState, token: String, user: UserDto| {
        st.persist_creds(&token, &user);
        pending_user.set(Some(user.clone()));
        spawn_local(async move {
            match api::list_workspaces().await {
                Ok(list) => {
                    st.workspaces.set(list.clone());
                    if list.is_empty() {
                        step2.set(true);
                    } else if let Some(u) = pending_user.get_untracked() {
                        st.user.set(Some(u));
                    }
                }
                Err(e) => {
                    st.toast(&format!("获取备考空间失败：{}", e));
                    if let Some(u) = pending_user.get_untracked() {
                        st.user.set(Some(u));
                    }
                }
            }
        });
    };

    let do_login = move |_| {
        let a = account.get_untracked().trim().to_string();
        let p = password.get_untracked();
        if a.is_empty() {
            state.toast("请输入账号");
            return;
        }
        if p.is_empty() {
            state.toast("请输入密码");
            return;
        }
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let st = state;
        spawn_local(async move {
            match api::login(&LoginRequest {
                account: a,
                password: p,
            })
            .await
            {
                Ok(resp) => after_auth(st, resp.token, resp.user),
                Err(e) => st.toast(&e.to_string()),
            }
            busy.set(false);
        });
    };

    let do_register = move |_| {
        let acc = reg_account.get_untracked().trim().to_string();
        let p1 = reg_password.get_untracked();
        let p2 = reg_password2.get_untracked();
        if acc.is_empty() {
            state.toast("请输入账号");
            return;
        }
        if p1.len() < 6 {
            state.toast("密码至少 6 位");
            return;
        }
        if p1 != p2 {
            state.toast("两次输入的密码不一致");
            return;
        }
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let st = state;
        spawn_local(async move {
            match api::register(&RegisterRequest {
                account: acc.clone(),
                password: p1,
                nickname: None,
            })
            .await
            {
                Ok(resp) => {
                    st.toast("注册成功，请登录");
                    account.set(acc);
                    password.set(String::new());
                    tab.set(AuthTab::Login);
                    after_auth(st, resp.token, resp.user);
                }
                Err(e) => st.toast(&e.to_string()),
            }
            busy.set(false);
        });
    };

    let confirm_setup = move |_| {
        let g = goal.get_untracked().trim().to_string();
        if g.is_empty() {
            state.toast("请手写填写你的考试目标");
            return;
        }
        let d = date.get_untracked();
        let d = if d.is_empty() {
            "2026-11-07".to_string()
        } else {
            d
        };
        let exam_date = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let st = state;
        let goal2 = g.clone();
        spawn_local(async move {
            match api::create_workspace(&WorkspaceInput {
                name: g.clone(),
                exam_goal: g.clone(),
                exam_date,
            })
            .await
            {
                Ok(ws) => {
                    st.set_workspace(&ws);
                    st.toast(&format!("已创建备考空间「{}」", goal2));
                    if let Some(user) = pending_user.get_untracked() {
                        st.user.set(Some(user));
                        let w = gloo_timers::callback::Timeout::new(700, move || {
                            st.agent_open.set(true);
                        });
                        w.forget();
                    }
                }
                Err(e) => st.toast(&e.to_string()),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="auth-page">
            <div class="auth-deco auth-deco-1"></div>
            <div class="auth-deco auth-deco-2"></div>
            <div class="auth-wrap">
                <div class="auth-brand">
                    <div class="logo" style="margin-bottom:28px;">
                        <div class="logo-icon" style="width:46px;height:46px;font-size:23px;border-radius:14px;">学</div>
                        <div>
                            <div class="logo-name" style="font-size:20px;">学伴</div>
                            <div class="logo-sub">超级学习助手</div>
                        </div>
                    </div>
                    <h1 class="auth-headline">"看课、刷题、备考"<br/>"一个空间就够了"</h1>
                    <p class="auth-sub">"把课程交给 AI：自动生成笔记与习题；把练习交给系统：刷题、错题、组卷、复盘一气呵成。"</p>
                    <div class="auth-features">
                        <div class="auth-feature">
                            <span class="af-icon">"🤖"</span>
                            <div><b>"AI 生成学习内容"</b><span class="af-desc">"目录 / 笔记 / 批注 / 习题，由 Agent 自动写入你的备考空间"</span></div>
                        </div>
                        <div class="auth-feature">
                            <span class="af-icon">"✏️"</span>
                            <div><b>"刷题与批注交互"</b><span class="af-desc">"本地判分即时反馈，正文随选随批注"</span></div>
                        </div>
                        <div class="auth-feature">
                            <span class="af-icon">"🩹"</span>
                            <div><b>"错题自动归集"</b><span class="af-desc">"错题本 + 组卷模考，补弱闭环直到考前"</span></div>
                        </div>
                        <div class="auth-feature">
                            <span class="af-icon">"🔌"</span>
                            <div><b>"任意 Agent 接入"</b><span class="af-desc">"复制凭证即可装配能力，数据按用户严格隔离"</span></div>
                        </div>
                    </div>
                    <div class="auth-flow">"看课 → AI 笔记 → 刷题 → 错题复盘 → 考前冲刺"</div>
                </div>

                <div class="auth-card">
                    <Show when=move || !step2.get() fallback=move || {
                        view! {
                            <div>
                                <div class="auth-steps">
                                    <span class="a-step done">"① 账号登录 ✓"</span>
                                    <span class="a-line"></span>
                                    <span class="a-step cur">"② 创建备考空间"</span>
                                </div>
                                <div class="modal-title" style="margin-bottom:4px;">"🎯 写下你的考试目标"</div>
                                <div class="modal-sub" style="margin-bottom:18px;">"自由填写，写清楚就行，不设下拉选项"</div>
                                <div class="form-row">
                                    <label class="form-label">"考试目标（手写填写）"</label>
                                    <input class="form-input" placeholder="如：软考 · 系统架构设计师"
                                        prop:value=move || goal.get()
                                        on:input=move |ev| goal.set(event_target_value(&ev)) />
                                </div>
                                <div class="form-row">
                                    <label class="form-label">"考试日期"</label>
                                    <input class="form-input" type="date"
                                        prop:value=move || date.get()
                                        on:input=move |ev| date.set(event_target_value(&ev)) />
                                </div>
                                <button class="btn btn-primary" style="width:100%;justify-content:center;padding:11px 0;margin-top:6px;"
                                    on:click=confirm_setup>
                                    "确认并进入系统"
                                </button>
                                <div class="auth-agree">
                                    "进入后将：创建备考空间 · 开启考试倒计时 · 弹出 Agent 接入凭证"
                                    <a class="auth-link" on:click=move |_| step2.set(false)>"← 返回登录"</a>
                                </div>
                            </div>
                        }
                    }>
                        <div>
                            <div class="auth-tabs">
                                <div class:auth-tab=true class:active=move || tab.get() == AuthTab::Login
                                    on:click=move |_| tab.set(AuthTab::Login)>"登录"</div>
                                <div class:auth-tab=true class:active=move || tab.get() == AuthTab::Register
                                    on:click=move |_| tab.set(AuthTab::Register)>"注册"</div>
                            </div>
                            <Show when=move || tab.get() == AuthTab::Login fallback=move || {
                                view! {
                                    <div>
                                        <div class="form-row">
                                            <label class="form-label">账号</label>
                                            <input class="form-input" placeholder="手机号 / 邮箱 / 用户名"
                                                prop:value=move || reg_account.get()
                                                on:input=move |ev| reg_account.set(event_target_value(&ev)) />
                                        </div>
                                        <div class="form-row">
                                            <label class="form-label">密码</label>
                                            <input class="form-input" type="password" placeholder="至少 6 位"
                                                prop:value=move || reg_password.get()
                                                on:input=move |ev| reg_password.set(event_target_value(&ev)) />
                                        </div>
                                        <div class="form-row">
                                            <label class="form-label">确认密码</label>
                                            <input class="form-input" type="password" placeholder="再次输入密码"
                                                prop:value=move || reg_password2.get()
                                                on:input=move |ev| reg_password2.set(event_target_value(&ev)) />
                                        </div>
                                        <button class="btn btn-primary" style="width:100%;justify-content:center;padding:11px 0;margin-top:6px;"
                                            on:click=do_register>
                                            "注 册"
                                        </button>
                                    </div>
                                }
                            }>
                                <div>
                                    <div class="form-row">
                                        <label class="form-label">账号</label>
                                        <input class="form-input" placeholder="手机号 / 邮箱 / 用户名"
                                            prop:value=move || account.get()
                                            on:input=move |ev| account.set(event_target_value(&ev)) />
                                    </div>
                                    <div class="form-row">
                                        <label class="form-label">密码</label>
                                        <input class="form-input" type="password" placeholder="输入密码"
                                            prop:value=move || password.get()
                                            on:input=move |ev| password.set(event_target_value(&ev)) />
                                    </div>
                                    <button class="btn btn-primary" style="width:100%;justify-content:center;padding:11px 0;margin-top:6px;"
                                        on:click=do_login>
                                        "登 录"
                                    </button>
                                </div>
                            </Show>
                            <div class="auth-agree">"账号体系是订阅计费与学习数据存储的基础<br>登录即代表同意《用户协议》与《隐私政策》（演示文案）"</div>
                        </div>
                    </Show>
                </div>
            </div>
            <div class="auth-footer">"学伴 · 超级学习助手 — 让 AI 陪你备考（演示原型）"</div>
        </div>
    }
}
