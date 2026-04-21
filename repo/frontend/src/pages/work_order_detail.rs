use gloo_net::http::Method;
use uuid::Uuid;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::{toast_err, toast_ok, AuthCtx, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::components::sla::SlaCountdown;
use crate::components::state_badge::{PriorityBadge, StateBadge};
use crate::offline;
use crate::routes::Route;
use crate::types::{DataEnvelope, RecipeStep, StepProgress, StepProgressStatus, WorkOrder, WorkOrderState};

#[derive(Properties, PartialEq)]
pub struct DetailProps {
    pub id: Uuid,
}

#[function_component(WorkOrderDetail)]
pub fn work_order_detail(props: &DetailProps) -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let wo = use_state(|| None::<WorkOrder>);
    let steps = use_state(Vec::<RecipeStep>::new);
    let progress = use_state(Vec::<StepProgress>::new);
    let loading = use_state(|| true);
    let transitioning = use_state(|| false);
    let reload = use_state(|| 0u32);

    {
        let wo = wo.clone();
        let steps = steps.clone();
        let progress = progress.clone();
        let loading = loading.clone();
        let auth = auth.clone();
        let toasts = toasts.clone();
        let id = props.id;
        let reload_dep = *reload;
        use_effect_with(reload_dep, move |_| {
            let state = auth.state.clone();
            loading.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                // Offline-first reads: cached GETs keep the detail view usable
                // even when the technician's tablet drops connectivity mid-job.
                match offline::get_cached::<WorkOrder>(&format!("/api/work-orders/{}", id), &state).await {
                    Ok(w) => {
                        let recipe_id = w.recipe_id;
                        wo.set(Some(w));
                        if let Some(rid) = recipe_id {
                            if let Ok(env) = offline::get_cached::<DataEnvelope<RecipeStep>>(
                                &format!("/api/recipes/{}/steps", rid),
                                &state,
                            )
                            .await
                            {
                                steps.set(env.data);
                            }
                        }
                        if let Ok(env) = offline::get_cached::<DataEnvelope<StepProgress>>(
                            &format!("/api/work-orders/{}/progress", id),
                            &state,
                        )
                        .await
                        {
                            progress.set(env.data);
                        }
                    }
                    Err(e) => toast_err(&toasts, format!("Failed to load job: {}", e.message)),
                }
                loading.set(false);
            });
            || ()
        });
    }

    let bump_reload = {
        let reload = reload.clone();
        Callback::from(move |_| reload.set(*reload + 1))
    };

    html! {
        <div class="stack">
            if *loading && wo.is_none() {
                <div class="empty-state">{ "Loading..." }</div>
            }
            if let Some(w) = wo.as_ref() {
                <div class="space-between">
                    <div>
                        <h1>{ &w.title }</h1>
                        <div class="row">
                            <StateBadge state={w.state.clone()} />
                            <PriorityBadge priority={w.priority.clone()} />
                            <SlaCountdown deadline={w.sla_deadline} started={Some(w.created_at)} />
                        </div>
                    </div>
                    <div class="row">
                        <Link<Route> to={Route::MapView { id: w.id }}>
                            <button class="ghost">{ "Map & Trail" }</button>
                        </Link<Route>>
                    </div>
                </div>

                if let Some(desc) = &w.description {
                    <div class="card"><p>{ desc }</p></div>
                }

                <CheckInPanel
                    wo={w.clone()}
                    on_changed={bump_reload.clone()}
                    toasts={toasts.clone()}
                    auth={auth.clone()}
                />

                <TransitionPanel
                    wo={w.clone()}
                    on_changed={bump_reload.clone()}
                    toasts={toasts.clone()}
                    auth={auth.clone()}
                    transitioning={transitioning.clone()}
                />

                <div class="card">
                    <h3>{ "Recipe steps" }</h3>
                    if steps.is_empty() {
                        <p class="muted">{ "No recipe attached to this work order." }</p>
                    } else {
                        <div class="stepper">
                            { for steps.iter().map(|s| {
                                let status = progress.iter().find(|p| p.step_id == s.id)
                                    .map(|p| p.status.clone())
                                    .unwrap_or(StepProgressStatus::Pending);
                                html!{ <StepRow step={s.clone()} wo_id={w.id} status={status} /> }
                            }) }
                        </div>
                    }
                </div>

                <Timeline wo_id={w.id} />
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct StepRowProps {
    step: RecipeStep,
    wo_id: Uuid,
    status: StepProgressStatus,
}

#[function_component(StepRow)]
fn step_row(props: &StepRowProps) -> Html {
    let nav = use_navigator().expect("navigator");
    let done = matches!(props.status, StepProgressStatus::Completed);
    let classes = if done { "step-item done" } else { "step-item" };
    let onclick = {
        let id = props.wo_id;
        let step_id = props.step.id;
        let nav = nav.clone();
        Callback::from(move |_| nav.push(&Route::RecipeStep { id, step_id }))
    };
    html! {
        <div class={classes} onclick={onclick}>
            <span class="num">{ props.step.step_order }</span>
            <div style="flex:1;">
                <div>{ &props.step.title }</div>
                <div class="small muted">{ props.status.label() }</div>
            </div>
            <span class="small muted">{ "›" }</span>
        </div>
    }
}

// Parse a decimal string into f64, accepting an empty string as None.
fn parse_coord(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<f64>().ok()
    }
}

// -----------------------------------------------------------------------------
// CheckInPanel — arrival/departure check-in buttons (PRD §7). Must be posted
// BEFORE the corresponding state transition (OnSite / Completed) or the server
// rejects the transition.
// -----------------------------------------------------------------------------
#[derive(Properties, PartialEq)]
struct CheckInPanelProps {
    wo: WorkOrder,
    on_changed: Callback<()>,
    toasts: crate::app::ToastCtx,
    auth: AuthCtx,
}

#[function_component(CheckInPanel)]
fn check_in_panel(props: &CheckInPanelProps) -> Html {
    let posting = use_state(|| false);
    let lat_in = use_state(|| {
        props.wo.location_lat.map(|v| format!("{:.6}", v)).unwrap_or_default()
    });
    let lng_in = use_state(|| {
        props.wo.location_lng.map(|v| format!("{:.6}", v)).unwrap_or_default()
    });

    let on_lat = {
        let lat_in = lat_in.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                lat_in.set(el.value());
            }
        })
    };
    let on_lng = {
        let lng_in = lng_in.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                lng_in.set(el.value());
            }
        })
    };

    let post_check_in = {
        let auth = props.auth.clone();
        let toasts = props.toasts.clone();
        let posting = posting.clone();
        let on_changed = props.on_changed.clone();
        let id = props.wo.id;
        let lat_in = lat_in.clone();
        let lng_in = lng_in.clone();
        Callback::from(move |kind: &'static str| {
            if *posting {
                return;
            }
            let lat = parse_coord(&lat_in);
            let lng = parse_coord(&lng_in);
            let (lat, lng) = match (lat, lng) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    toast_err(
                        &toasts,
                        "Check-in requires numeric lat and lng".to_string(),
                    );
                    return;
                }
            };
            posting.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let posting = posting.clone();
            let on_changed = on_changed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({
                    "type": kind,
                    "lat": lat,
                    "lng": lng,
                });
                let url = format!("/api/work-orders/{}/check-in", id);
                // mutate_with_queue returns Ok(None) when the request was
                // deferred to the offline queue — surface that distinctly so
                // the tech knows the write will converge on reconnect.
                match offline::mutate_with_queue(Method::POST, &url, &body, &state).await {
                    Ok(Some(_)) => {
                        toast_ok(&toasts, format!("{} check-in recorded", kind));
                        on_changed.emit(());
                    }
                    Ok(None) => {
                        toast_ok(&toasts, format!("{} check-in queued (offline)", kind));
                        on_changed.emit(());
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                posting.set(false);
            });
        })
    };

    let on_arrival = {
        let cb = post_check_in.clone();
        Callback::from(move |_| cb.emit("ARRIVAL"))
    };
    let on_departure = {
        let cb = post_check_in.clone();
        Callback::from(move |_| cb.emit("DEPARTURE"))
    };

    html! {
        <div class="card">
            <h3>{ "Check-ins" }</h3>
            <p class="small muted">
                { "Record an ARRIVAL before moving to On Site. Record a DEPARTURE before Completing." }
            </p>
            <div class="row">
                <label>
                    { "Lat" }
                    <input type="text" value={(*lat_in).clone()} oninput={on_lat} />
                </label>
                <label>
                    { "Lng" }
                    <input type="text" value={(*lng_in).clone()} oninput={on_lng} />
                </label>
            </div>
            <div class="row" style="margin-top: 8px;">
                <LoadingButton
                    label="Arrival check-in"
                    loading={*posting}
                    onclick={on_arrival}
                    kind={Some("secondary".to_string())}
                />
                <LoadingButton
                    label="Departure check-in"
                    loading={*posting}
                    onclick={on_departure}
                    kind={Some("secondary".to_string())}
                />
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct TransitionPanelProps {
    wo: WorkOrder,
    on_changed: Callback<()>,
    toasts: crate::app::ToastCtx,
    auth: AuthCtx,
    transitioning: UseStateHandle<bool>,
}

#[function_component(TransitionPanel)]
fn transition_panel(props: &TransitionPanelProps) -> Html {
    let notes = use_state(String::new);
    // Pre-fill from the work order's recorded location so EnRoute transitions
    // have a sensible default. The user can override.
    let lat_in = use_state(|| {
        props.wo.location_lat.map(|v| format!("{:.6}", v)).unwrap_or_default()
    });
    let lng_in = use_state(|| {
        props.wo.location_lng.map(|v| format!("{:.6}", v)).unwrap_or_default()
    });

    let available: Vec<WorkOrderState> = match props.wo.state {
        WorkOrderState::Scheduled => vec![WorkOrderState::EnRoute, WorkOrderState::Canceled],
        WorkOrderState::EnRoute => vec![WorkOrderState::OnSite, WorkOrderState::Canceled],
        WorkOrderState::OnSite => vec![WorkOrderState::InProgress, WorkOrderState::Canceled],
        WorkOrderState::InProgress => vec![
            WorkOrderState::WaitingOnParts,
            WorkOrderState::Completed,
            WorkOrderState::Canceled,
        ],
        WorkOrderState::WaitingOnParts => {
            vec![WorkOrderState::InProgress, WorkOrderState::Canceled]
        }
        WorkOrderState::Completed | WorkOrderState::Canceled => vec![],
    };

    let on_notes = {
        use wasm_bindgen::JsCast;
        let notes = notes.clone();
        Callback::from(move |e: InputEvent| {
            let t = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok());
            if let Some(el) = t {
                notes.set(el.value());
            }
        })
    };

    let on_lat = {
        let lat_in = lat_in.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                lat_in.set(el.value());
            }
        })
    };
    let on_lng = {
        let lng_in = lng_in.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                lng_in.set(el.value());
            }
        })
    };

    let transition = {
        let toasts = props.toasts.clone();
        let auth = props.auth.clone();
        let transitioning = props.transitioning.clone();
        let on_changed = props.on_changed.clone();
        let notes = notes.clone();
        let lat_in = lat_in.clone();
        let lng_in = lng_in.clone();
        let id = props.wo.id;
        Callback::from(move |to: WorkOrderState| {
            if *transitioning {
                return;
            }
            // Pre-validate EnRoute lat/lng locally so users see a clear error.
            if matches!(to, WorkOrderState::EnRoute) {
                if parse_coord(&lat_in).is_none() || parse_coord(&lng_in).is_none() {
                    toast_err(
                        &toasts,
                        "EnRoute requires numeric lat/lng — fill the fields first"
                            .to_string(),
                    );
                    return;
                }
            }
            transitioning.set(true);
            let toasts = toasts.clone();
            let state = auth.state.clone();
            let transitioning = transitioning.clone();
            let on_changed = on_changed.clone();
            let notes_val = (*notes).clone();
            let lat = parse_coord(&lat_in);
            let lng = parse_coord(&lng_in);
            notes.set(String::new());
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({
                    "to_state": to,
                    "notes": if notes_val.is_empty() { None } else { Some(notes_val) },
                    "lat": lat,
                    "lng": lng,
                });
                let url = format!("/api/work-orders/{}/state", id);
                match offline::mutate_with_queue(Method::PUT, &url, &body, &state).await {
                    Ok(Some(_)) => {
                        toast_ok(&toasts, format!("Moved to {:?}", to));
                        on_changed.emit(());
                    }
                    Ok(None) => {
                        toast_ok(&toasts, format!("{:?} transition queued (offline)", to));
                        on_changed.emit(());
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                transitioning.set(false);
            });
        })
    };

    if available.is_empty() {
        return html! {
            <div class="card"><p class="muted">{ "This work order is in a terminal state." }</p></div>
        };
    }

    html! {
        <div class="card">
            <h3>{ "Actions" }</h3>
            <div class="row">
                <label>
                    { "Lat (required for En Route)" }
                    <input type="text" value={(*lat_in).clone()} oninput={on_lat} />
                </label>
                <label>
                    { "Lng (required for En Route)" }
                    <input type="text" value={(*lng_in).clone()} oninput={on_lng} />
                </label>
            </div>
            <label>
                { "Notes (required for Cancel / WaitingOnParts)" }
                <textarea value={(*notes).clone()} oninput={on_notes} />
            </label>
            <div class="row" style="margin-top: 12px;">
                { for available.iter().cloned().map(|s| {
                    let cb = transition.clone();
                    let label = format!("→ {}", s.label());
                    let kind = if matches!(s, WorkOrderState::Canceled) { Some("danger".to_string()) } else { None };
                    let onclick = Callback::from(move |_| cb.emit(s.clone()));
                    html! {
                        <LoadingButton
                            label={label}
                            loading={*props.transitioning}
                            onclick={onclick}
                            kind={kind}
                        />
                    }
                }) }
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct TimelineProps {
    wo_id: Uuid,
}

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct TimelineEntry {
    from_state: Option<String>,
    to_state: String,
    notes: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct TimelineResp {
    data: Vec<TimelineEntry>,
}

#[function_component(Timeline)]
fn timeline(props: &TimelineProps) -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let entries = use_state(Vec::<TimelineEntry>::new);
    {
        let entries = entries.clone();
        let auth = auth.clone();
        let id = props.wo_id;
        use_effect_with(id, move |_| {
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(r) = offline::get_cached::<TimelineResp>(
                    &format!("/api/work-orders/{}/timeline", id),
                    &state,
                )
                .await
                {
                    entries.set(r.data);
                }
            });
            || ()
        });
    }

    html! {
        <div class="card">
            <h3>{ "Timeline" }</h3>
            if entries.is_empty() {
                <p class="muted">{ "No transitions yet." }</p>
            } else {
                <ul>
                    { for entries.iter().map(|e| html!{
                        <li>
                            <strong>{ e.from_state.clone().unwrap_or_else(|| "—".into()) }{ " → " }{ &e.to_state }</strong>
                            { " · " }<span class="small muted">{ e.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string() }</span>
                            if let Some(n) = &e.notes { <div class="small muted">{ n }</div> }
                        </li>
                    }) }
                </ul>
            }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn parse_coord_returns_none_for_empty_input() {
        // The check-in and transition forms treat blank fields as "not set"
        // so the downstream validator can fail fast with a clear message.
        assert_eq!(parse_coord(""), None);
        assert_eq!(parse_coord("   "), None);
    }

    #[wasm_bindgen_test]
    fn parse_coord_accepts_signed_decimals() {
        assert_eq!(parse_coord("37.7749"),  Some(37.7749));
        assert_eq!(parse_coord("-122.4194"), Some(-122.4194));
        assert_eq!(parse_coord("0"),         Some(0.0));
        // Surrounding whitespace is tolerated — technicians paste values from
        // the map panel, which sometimes includes a trailing space.
        assert_eq!(parse_coord("  12.5 "),   Some(12.5));
    }

    #[wasm_bindgen_test]
    fn parse_coord_rejects_garbage() {
        assert_eq!(parse_coord("north"),  None);
        assert_eq!(parse_coord("12.3.4"), None);
        assert_eq!(parse_coord("12,3"),   None); // comma decimal — locale mismatch
    }
}

