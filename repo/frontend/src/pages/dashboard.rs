use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::{toast_err, AuthCtx, ToastCtx};
use crate::components::sla::SlaCountdown;
use crate::components::state_badge::{PriorityBadge, StateBadge};
use crate::offline;
use crate::routes::Route;
use crate::types::{Paginated, WorkOrder};

#[function_component(Dashboard)]
pub fn dashboard() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");

    let rows = use_state(|| None::<Paginated<WorkOrder>>);
    let loading = use_state(|| true);
    let err = use_state(|| None::<String>);

    {
        let rows = rows.clone();
        let loading = loading.clone();
        let err = err.clone();
        let auth = auth.clone();
        let toasts = toasts.clone();
        use_effect_with((), move |_| {
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                // Offline-first: offline::get_cached serves a cached copy when
                // the network drops so the technician's job list still loads.
                match offline::get_cached::<Paginated<WorkOrder>>(
                    "/api/work-orders?per_page=50",
                    &state,
                )
                .await
                {
                    Ok(p) => rows.set(Some(p)),
                    Err(e) => {
                        err.set(Some(e.message.clone()));
                        toast_err(&toasts, format!("Failed to load jobs: {}", e.message));
                    }
                }
                loading.set(false);
            });
            || ()
        });
    }

    let heading = auth
        .state
        .user
        .as_ref()
        .map(|u| format!("{}'s jobs", u.username))
        .unwrap_or_else(|| "Jobs".into());

    html! {
        <div class="stack">
            <div class="space-between">
                <h1>{ heading }</h1>
                <span class="muted">
                    { rows.as_ref().map(|p| format!("{} total", p.total)).unwrap_or_default() }
                </span>
            </div>

            if *loading {
                <div class="empty-state">{ "Loading jobs..." }</div>
            } else if let Some(msg) = err.as_ref() {
                <div class="error-banner">{ msg }</div>
            } else if let Some(p) = rows.as_ref() {
                if p.data.is_empty() {
                    <div class="empty-state">{ "No work orders yet." }</div>
                } else {
                    <div class="grid">
                        { for p.data.iter().map(|wo| html!{ <JobCard wo={wo.clone()} /> }) }
                    </div>
                }
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct JobCardProps {
    wo: WorkOrder,
}

#[function_component(JobCard)]
fn job_card(props: &JobCardProps) -> Html {
    let wo = &props.wo;
    let nav = use_navigator().expect("navigator");
    let onclick = {
        let id = wo.id;
        let nav = nav.clone();
        Callback::from(move |_| nav.push(&Route::WorkOrder { id }))
    };
    html! {
        <a class="wo-card" onclick={onclick}>
            <div class="title">{ &wo.title }</div>
            <div class="meta">
                <StateBadge state={wo.state.clone()} />
                <PriorityBadge priority={wo.priority.clone()} />
                if let Some(addr) = &wo.location_address_norm {
                    <span>{ addr }</span>
                }
            </div>
            <div class="footer">
                <SlaCountdown deadline={wo.sla_deadline} started={Some(wo.created_at)} />
                <span class="right small muted">{ format!("v{}", wo.version_count) }</span>
            </div>
        </a>
    }
}
