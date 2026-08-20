//! 学伴 Web 客户端（Leptos 0.7 CSR，编译 WASM，浏览器与 Tauri 共用）。

pub mod api;
pub mod markdown;
pub mod state;
pub mod views;

use leptos::prelude::*;

#[component]
fn Root() -> impl IntoView {
    let state = state::AppState::new();
    crate::api::set_unauthorized_handler(move || {
        state.clear_auth();
        state.toast("登录已失效，请重新登录");
    });
    provide_context(state);
    // 无感登录：启动时用本地 token 校验并刷新会话（失败按 unauthorized 回调清理）。
    leptos::task::spawn_local(async move {
        state.restore_session().await;
    });
    view! {
        <Show
            when=move || state.entered.get()
            fallback=move || view! { <views::auth::AuthPage /> }
        >
            <views::shell::Shell />
        </Show>
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn mount() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(|| view! { <Root /> });
}

/// wasm 实例化后自动挂载（trunk 的注入脚本只 await init，不会调用 mount）。
#[wasm_bindgen::prelude::wasm_bindgen(start)]
fn start() {
    mount();
}

/// Tauri 薄壳引用的应用名。
pub fn app_name() -> &'static str {
    "学伴"
}
