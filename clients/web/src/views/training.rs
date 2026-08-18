//! 训练打卡视图（体育领域包）：打卡表单 + 历史列表。
//!
//! 表单校验在前端做一遍（读信号 → 校验 → busy 防重 → spawn_local），
//! 提交成功后刷新列表；后端仍做完整校验（Invalid → 400）。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, CheckinInput};
use crate::state::{fmt_date_ymd, AppState};

/// sport 值 → 显示名；未知值原样展示（后端不做枚举约束，向前兼容）。
pub fn sport_label(sport: &str) -> String {
    match sport {
        "badminton" => "羽毛球".to_string(),
        "core" => "核心训练".to_string(),
        other => other.to_string(),
    }
}

/// 拉取打卡历史列表，更新状态；失败 toast（与错题本 refresh_wrong 同风格）。
pub async fn refresh_checkins(state: AppState) {
    match api::training_checkins(50).await {
        Ok(list) => state.checkins.set(list),
        Err(e) => state.toast(&format!("打卡记录加载失败：{}", e)),
    }
}

#[component]
pub fn TrainingView(state: AppState) -> impl IntoView {
    let sport = RwSignal::new("badminton".to_string());
    let activity = RwSignal::new(String::new());
    let duration = RwSignal::new(String::new());
    let rating = RwSignal::new(0u8);
    let note = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let do_checkin = move |_| {
        let sp = sport.get_untracked();
        let act = activity.get_untracked().trim().to_string();
        if act.is_empty() {
            state.toast("请输入训练内容");
            return;
        }
        let minutes: u32 = match duration.get_untracked().trim().parse() {
            Ok(m) if m > 0 => m,
            _ => {
                state.toast("训练时长必须是大于 0 的整数（分钟）");
                return;
            }
        };
        let rt = rating.get_untracked();
        if !(1..=5).contains(&rt) {
            state.toast("请选择 1~5 星自评");
            return;
        }
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let st = state;
        let input = CheckinInput {
            sport: sp,
            activity: act,
            duration_minutes: minutes,
            rating: rt,
            note: {
                let n = note.get_untracked().trim().to_string();
                if n.is_empty() {
                    None
                } else {
                    Some(n)
                }
            },
        };
        spawn_local(async move {
            match api::checkin(&input).await {
                Ok(_) => {
                    st.toast("打卡成功");
                    activity.set(String::new());
                    duration.set(String::new());
                    rating.set(0);
                    note.set(String::new());
                    refresh_checkins(st).await;
                }
                Err(e) => st.toast(&format!("打卡失败：{}", e)),
            }
            busy.set(false);
        });
    };

    let stars = move || {
        let cur = rating.get();
        (1..=5)
            .map(|i| {
                let filled = i <= cur;
                (view! {
                    <span class="rating-star" class:active=filled
                        on:click=move |_| rating.set(i)>
                        {if filled { "★" } else { "☆" }}
                    </span>
                })
                .into_any()
            })
            .collect::<Vec<_>>()
    };

    let rows = move || {
        let list = state.checkins.get();
        if list.is_empty() {
            return vec![(view! {
                <div style="padding:16px 10px;font-size:12.5px;color:var(--muted-light)">
                    "还没有打卡记录，练完一次就记一笔吧"
                </div>
            })
            .into_any()];
        }
        list.into_iter()
            .map(|r| {
                let date = fmt_date_ymd(r.created_at);
                let stars = "★".repeat(r.rating as usize);
                (view! {
                    <div class="wrong-item">
                        <div class="wrong-icon">"🏸"</div>
                        <div class="wrong-content">
                            <div class="wrong-question">
                                {sport_label(&r.sport)}
                                " · "
                                {r.activity.clone()}
                                " · "
                                {r.duration_minutes}
                                " 分钟"
                            </div>
                            <div class="wrong-meta">
                                <span>{date}</span>
                                <span class="training-stars">{stars}</span>
                                {r.note.clone().map(|n| (view! { <span>{n}</span> }).into_any())}
                            </div>
                        </div>
                    </div>
                })
                .into_any()
            })
            .collect()
    };

    view! {
        <div class="training-view">
            <div class="training-card">
                <div class="src-hint">"💡 打卡记录写入事件流，AI 复盘时可按运动 / 时长 / 自评定位薄弱环节"</div>
                <div class="form-row">
                    <div class="form-label">"运动项目"</div>
                    <div class="training-sports">
                        <div class="filter-chip" class:active=move || sport.get() == "badminton"
                            on:click=move |_| sport.set("badminton".to_string())>
                            "羽毛球"
                        </div>
                        <div class="filter-chip" class:active=move || sport.get() == "core"
                            on:click=move |_| sport.set("core".to_string())>
                            "核心训练"
                        </div>
                    </div>
                </div>
                <div class="form-row">
                    <div class="form-label">"训练内容"</div>
                    <input class="form-input" type="text" placeholder="如：正手高远球 / 平板支撑"
                        prop:value=move || activity.get()
                        on:input=move |ev| activity.set(event_target_value(&ev)) />
                </div>
                <div class="form-row">
                    <div class="form-label">"训练时长（分钟）"</div>
                    <input class="form-input" type="number" min="1" placeholder="如：60"
                        prop:value=move || duration.get()
                        on:input=move |ev| duration.set(event_target_value(&ev)) />
                </div>
                <div class="form-row">
                    <div class="form-label">"本次自评"</div>
                    <div class="training-stars">{stars}</div>
                </div>
                <div class="form-row">
                    <div class="form-label">"备注（可选）"</div>
                    <input class="form-input" type="text" placeholder="如：今天手感不错，发力更顺了"
                        prop:value=move || note.get()
                        on:input=move |ev| note.set(event_target_value(&ev)) />
                </div>
                <button class="btn btn-primary" disabled=move || busy.get()
                    on:click=do_checkin>
                    {move || if busy.get() { "提交中…" } else { "打卡" }}
                </button>
            </div>

            <div class="training-history">
                <div class="section-title">"打卡历史"</div>
                <div class="wrong-list">{rows}</div>
            </div>
        </div>
    }
}
