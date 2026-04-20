use gloo_net::http::Method;
use std::collections::HashMap;
use uuid::Uuid;
use wasm_bindgen::JsCast;
use web_sys::HtmlTextAreaElement;
use yew::prelude::*;

use crate::app::{toast_err, toast_ok, AuthCtx, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::components::timer_ring::{TimerRing, TimerSnapshot};
use crate::offline;
use crate::types::{DataEnvelope, RecipeStep, StepProgress, StepProgressStatus, TipCard};

#[derive(Properties, PartialEq)]
pub struct StepProps {
    pub work_order_id: Uuid,
    pub step_id: Uuid,
}

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct StepTimer {
    id: Uuid,
    step_id: Uuid,
    label: String,
    duration_seconds: i32,
    alert_type: String,
}

#[function_component(RecipeStepPage)]
pub fn recipe_step_page(props: &StepProps) -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");

    let step = use_state(|| None::<RecipeStep>);
    let timers = use_state(Vec::<StepTimer>::new);
    let tips = use_state(Vec::<TipCard>::new);
    let progress = use_state(|| None::<StepProgress>);
    let notes = use_state(String::new);
    let saving = use_state(|| false);
    // Map of timer_id -> snapshot; hydrated from backend on load, mutated by
    // the TimerRing components via the `on_tick` callback.
    let snapshots = use_state(HashMap::<Uuid, TimerSnapshot>::new);

    {
        let auth = auth.clone();
        let step = step.clone();
        let timers = timers.clone();
        let tips = tips.clone();
        let progress = progress.clone();
        let notes = notes.clone();
        let snapshots = snapshots.clone();
        let wo_id = props.work_order_id;
        let step_id = props.step_id;
        use_effect_with((wo_id, step_id), move |_| {
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                // Offline-first GETs: the recipe-step screen is the hottest
                // path in the field; cached copies let the tech keep working
                // even when the truck rolls through a dead zone.
                if let Ok(wo) = offline::get_cached::<crate::types::WorkOrder>(
                    &format!("/api/work-orders/{}", wo_id),
                    &state,
                )
                .await
                {
                    if let Some(rid) = wo.recipe_id {
                        if let Ok(env) = offline::get_cached::<DataEnvelope<RecipeStep>>(
                            &format!("/api/recipes/{}/steps", rid),
                            &state,
                        )
                        .await
                        {
                            if let Some(s) = env.data.into_iter().find(|s| s.id == step_id) {
                                step.set(Some(s));
                            }
                        }
                    }
                }
                // Tip cards
                if let Ok(env) = offline::get_cached::<DataEnvelope<TipCard>>(
                    &format!("/api/steps/{}/tip-cards", step_id),
                    &state,
                )
                .await
                {
                    tips.set(env.data);
                }
                // Step timers — real backend-defined timers. Multiple concurrent
                // rings, each with its own duration and alert_type.
                if let Ok(env) = offline::get_cached::<DataEnvelope<StepTimer>>(
                    &format!("/api/steps/{}/timers", step_id),
                    &state,
                )
                .await
                {
                    timers.set(env.data);
                }
                // Progress + persisted timer snapshot: restore running state so
                // pause/resume across device boots works deterministically.
                if let Ok(env) = offline::get_cached::<DataEnvelope<StepProgressDetail>>(
                    &format!("/api/work-orders/{}/progress", wo_id),
                    &state,
                )
                .await
                {
                    if let Some(p) = env.data.into_iter().find(|p| p.step_id == step_id) {
                        notes.set(p.notes.clone().unwrap_or_default());
                        progress.set(Some(StepProgress {
                            id: p.id,
                            work_order_id: p.work_order_id,
                            step_id: p.step_id,
                            status: p.status.clone(),
                            notes: p.notes.clone(),
                            version: p.version,
                        }));
                        // Try to decode persisted per-timer snapshots.
                        if let Some(snap_val) = p.timer_state_snapshot {
                            if let Ok(list) = serde_json::from_value::<Vec<TimerSnapshot>>(snap_val) {
                                let mut map = HashMap::new();
                                for s in list {
                                    map.insert(s.timer_id, s);
                                }
                                snapshots.set(map);
                            }
                        }
                    }
                }
            });
            || ()
        });
    }

    let on_notes = {
        let notes = notes.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()) {
                notes.set(el.value());
            }
        })
    };

    let on_tick = {
        let snapshots = snapshots.clone();
        Callback::from(move |s: TimerSnapshot| {
            let mut m = (*snapshots).clone();
            m.insert(s.timer_id, s);
            snapshots.set(m);
        })
    };

    let set_status = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let saving = saving.clone();
        let notes = notes.clone();
        let progress = progress.clone();
        let snapshots = snapshots.clone();
        let wo_id = props.work_order_id;
        let step_id = props.step_id;
        Callback::from(move |status: StepProgressStatus| {
            if *saving {
                return;
            }
            saving.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let saving = saving.clone();
            let notes_val = (*notes).clone();
            let progress = progress.clone();
            // Collect current timer snapshot as a stable vector ordered by id.
            let mut snap_vec: Vec<TimerSnapshot> = snapshots.values().cloned().collect();
            snap_vec.sort_by_key(|s| s.timer_id);
            let timer_state = if snap_vec.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::to_value(&snap_vec).unwrap_or(serde_json::Value::Null)
            };
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({
                    "status": status,
                    "notes": if notes_val.is_empty() { None } else { Some(notes_val) },
                    "timer_state": timer_state,
                });
                let url = format!("/api/work-orders/{}/steps/{}/progress", wo_id, step_id);
                match offline::mutate_with_queue(Method::PUT, &url, &body, &state).await {
                    Ok(Some(v)) => {
                        if let Ok(p) = serde_json::from_value::<StepProgress>(v) {
                            progress.set(Some(p));
                        }
                        toast_ok(&toasts, "Progress saved");
                    }
                    Ok(None) => {
                        // Queued offline — server will resolve via the merge
                        // policy on reconnect. Keep local UI state coherent.
                        toast_ok(&toasts, "Progress queued (offline)");
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                saving.set(false);
            });
        })
    };

    let mk_button = |label: &str, s: StepProgressStatus, kind: Option<&str>| {
        let cb = set_status.clone();
        let onclick = Callback::from(move |_| cb.emit(s.clone()));
        html! {
            <LoadingButton
                label={label.to_string()}
                loading={*saving}
                onclick={onclick}
                kind={kind.map(String::from)}
            />
        }
    };

    html! {
        <div class="stack">
            if let Some(s) = step.as_ref() {
                <h1>{ format!("Step {}: {}", s.step_order, s.title) }</h1>
                if let Some(i) = &s.instructions {
                    <div class="card"><p>{ i }</p></div>
                }

                <div class="card">
                    <h3>{ "Concurrent timers" }</h3>
                    if timers.is_empty() {
                        <p class="small muted">
                            { "No timers defined for this step." }
                        </p>
                    } else {
                        <div class="row">
                            { for timers.iter().map(|t| {
                                let snap = snapshots.get(&t.id).cloned();
                                html!{
                                    <TimerRing
                                        timer_id={Some(t.id)}
                                        label={t.label.clone()}
                                        duration_seconds={t.duration_seconds.max(0) as u32}
                                        alert_type={t.alert_type.clone()}
                                        initial_remaining={snap.as_ref().map(|s| s.remaining_seconds.max(0) as u32)}
                                        initial_running={snap.map(|s| s.running).unwrap_or(false)}
                                        on_tick={on_tick.clone()}
                                    />
                                }
                            }) }
                        </div>
                    }
                </div>

                if !tips.is_empty() {
                    <div class="card">
                        <h3>{ "Tip cards" }</h3>
                        <div class="stack">
                            { for tips.iter().map(|t| html!{
                                <div class="tip-card">
                                    <h4>{ &t.title }</h4>
                                    <p>{ &t.content }</p>
                                </div>
                            }) }
                        </div>
                    </div>
                }

                <div class="card">
                    <label>
                        { "Notes (auto-saved on Pause / Complete)" }
                        <textarea value={(*notes).clone()} oninput={on_notes} />
                    </label>
                    <div class="row" style="margin-top: 12px;">
                        { mk_button("Start", StepProgressStatus::InProgress, None) }
                        { mk_button("Pause", StepProgressStatus::Paused, Some("secondary")) }
                        { mk_button("Mark complete", StepProgressStatus::Completed, None) }
                    </div>
                </div>
            } else {
                <div class="empty-state">{ "Loading step..." }</div>
            }
        </div>
    }
}

// Local, more complete version of StepProgress for decoding the timer
// snapshot field (the shared `StepProgress` in types.rs intentionally omits it
// to keep reads lean).
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
struct StepProgressDetail {
    id: Uuid,
    work_order_id: Uuid,
    step_id: Uuid,
    status: StepProgressStatus,
    notes: Option<String>,
    timer_state_snapshot: Option<serde_json::Value>,
    version: i32,
}
