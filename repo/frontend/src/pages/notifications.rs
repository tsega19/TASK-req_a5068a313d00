use yew::prelude::*;

use crate::api;
use crate::app::{toast_err, toast_ok, AuthCtx, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::types::{Notification, NotificationTemplate, Paginated};

#[function_component(NotificationsPage)]
pub fn notifications_page() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let rows = use_state(|| None::<Paginated<Notification>>);
    let loading = use_state(|| true);
    let reload = use_state(|| 0u32);

    {
        let rows = rows.clone();
        let loading = loading.clone();
        let auth = auth.clone();
        let toasts = toasts.clone();
        let dep = *reload;
        use_effect_with(dep, move |_| {
            loading.set(true);
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::get::<Paginated<Notification>>("/api/notifications", &state).await {
                    Ok(r) => rows.set(Some(r)),
                    Err(e) => toast_err(&toasts, format!("Load failed: {}", e.message)),
                }
                loading.set(false);
            });
            || ()
        });
    }

    let mark_read = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let reload = reload.clone();
        Callback::from(move |id: uuid::Uuid| {
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let reload = reload.clone();
            let reload_val = *reload;
            wasm_bindgen_futures::spawn_local(async move {
                let url = format!("/api/notifications/{}/read", id);
                match api::put::<_, serde_json::Value>(&url, &serde_json::json!({}), &state).await {
                    Ok(_) => reload.set(reload_val + 1),
                    Err(e) => toast_err(&toasts, e.message),
                }
            });
        })
    };

    let unsubscribing = use_state(|| None::<NotificationTemplate>);
    let unsubscribe = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let unsubscribing = unsubscribing.clone();
        Callback::from(move |tpl: NotificationTemplate| {
            unsubscribing.set(Some(tpl.clone()));
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let unsubscribing = unsubscribing.clone();
            let tpl_for_msg = tpl.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({ "template_type": tpl });
                match api::put::<_, serde_json::Value>("/api/notifications/unsubscribe", &body, &state).await {
                    Ok(_) => toast_ok(&toasts, format!("Unsubscribed from {}", tpl_for_msg.label())),
                    Err(e) => toast_err(&toasts, e.message),
                }
                unsubscribing.set(None);
            });
        })
    };

    html! {
        <div class="stack">
            <h1>{ "Notifications" }</h1>

            <div class="card">
                <h3>{ "Unsubscribe by template" }</h3>
                <div class="row">
                    { for NotificationTemplate::ALL.iter().cloned().map(|t| {
                        let cb = unsubscribe.clone();
                        let tpl_for_click = t.clone();
                        let onclick = Callback::from(move |_| cb.emit(tpl_for_click.clone()));
                        let loading = (*unsubscribing).as_ref() == Some(&t);
                        html!{
                            <LoadingButton
                                label={t.label().to_string()}
                                loading={loading}
                                onclick={onclick}
                                kind={Some("ghost".to_string())}
                            />
                        }
                    }) }
                </div>
            </div>

            if *loading {
                <div class="empty-state">{ "Loading..." }</div>
            } else if let Some(p) = rows.as_ref() {
                if p.data.is_empty() {
                    <div class="empty-state">{ "No notifications." }</div>
                } else {
                    <div class="stack">
                        { for p.data.iter().map(|n| {
                            let read = n.read_at.is_some();
                            let cls = if read { "notif-row" } else { "notif-row unread" };
                            let onclick = {
                                let mr = mark_read.clone();
                                let id = n.id;
                                Callback::from(move |_| if !read { mr.emit(id); })
                            };
                            html!{
                                <div class={cls} onclick={onclick}>
                                    <div class="body">
                                        <div class="template">{ n.template_type.label() }</div>
                                        <div>{ n.payload.to_string() }</div>
                                    </div>
                                    <span class="ts">{ n.created_at.format("%m/%d %H:%M").to_string() }</span>
                                </div>
                            }
                        }) }
                    </div>
                }
            }
        </div>
    }
}
