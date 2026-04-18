use wasm_bindgen::JsCast;
use web_sys::{Blob, HtmlAnchorElement, HtmlInputElement, HtmlSelectElement, Url};
use yew::prelude::*;

use crate::api;
use crate::app::{toast_err, toast_ok, AuthCtx, RoleGate, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::types::{DataEnvelope, LearningRow, Role};

#[function_component(AnalyticsPage)]
pub fn analytics_page() -> Html {
    html! {
        <RoleGate allowed={vec![Role::Super, Role::Admin]} fallback={html!{
            <div class="error-banner">{ "Analytics is for supervisors and admins only." }</div>
        }}>
            <AnalyticsBody />
        </RoleGate>
    }
}

#[function_component(AnalyticsBody)]
fn analytics_body() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");

    let from = use_state(String::new);
    let to = use_state(String::new);
    let role = use_state(String::new);
    let rows = use_state(Vec::<LearningRow>::new);
    let loading = use_state(|| false);
    let exporting = use_state(|| false);

    let build_query = {
        let from = from.clone();
        let to = to.clone();
        let role = role.clone();
        move || {
            let mut q = String::new();
            if !from.is_empty() {
                q.push_str(&format!("from={}&", *from));
            }
            if !to.is_empty() {
                q.push_str(&format!("to={}&", *to));
            }
            if !role.is_empty() {
                q.push_str(&format!("role={}&", *role));
            }
            if q.ends_with('&') {
                q.pop();
            }
            q
        }
    };

    let run_query = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let rows = rows.clone();
        let loading = loading.clone();
        let build_query = build_query.clone();
        Callback::from(move |_| {
            if *loading {
                return;
            }
            loading.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let rows = rows.clone();
            let loading = loading.clone();
            let q = build_query();
            let url = if q.is_empty() {
                "/api/analytics/learning".to_string()
            } else {
                format!("/api/analytics/learning?{}", q)
            };
            wasm_bindgen_futures::spawn_local(async move {
                match api::get::<DataEnvelope<LearningRow>>(&url, &state).await {
                    Ok(r) => rows.set(r.data),
                    Err(e) => toast_err(&toasts, format!("Query failed: {}", e.message)),
                }
                loading.set(false);
            });
        })
    };

    let export_csv = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let exporting = exporting.clone();
        let build_query = build_query.clone();
        Callback::from(move |_| {
            if *exporting {
                return;
            }
            exporting.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let exporting = exporting.clone();
            let q = build_query();
            let url = if q.is_empty() {
                "/api/analytics/learning/export-csv".to_string()
            } else {
                format!("/api/analytics/learning/export-csv?{}", q)
            };
            wasm_bindgen_futures::spawn_local(async move {
                match api::get_bytes(&url, &state).await {
                    Ok(bytes) => {
                        if let Err(e) = trigger_download(&bytes, "learning-analytics.csv") {
                            toast_err(&toasts, format!("Download failed: {}", e));
                        } else {
                            toast_ok(&toasts, "CSV downloaded");
                        }
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                exporting.set(false);
            });
        })
    };

    let on_from = text_input(from.clone());
    let on_to = text_input(to.clone());
    let on_role = {
        let role = role.clone();
        Callback::from(move |e: Event| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
                role.set(el.value());
            }
        })
    };

    html! {
        <div class="stack">
            <h1>{ "Learning analytics" }</h1>
            <div class="card">
                <div class="row">
                    <label>{ "From (MM/DD/YYYY)" }
                        <input type="text" value={(*from).clone()} oninput={on_from} placeholder="01/01/2026" />
                    </label>
                    <label>{ "To (MM/DD/YYYY)" }
                        <input type="text" value={(*to).clone()} oninput={on_to} placeholder="12/31/2026" />
                    </label>
                    <label>{ "Role" }
                        <select value={(*role).clone()} onchange={on_role}>
                            <option value="">{ "Any" }</option>
                            <option value="TECH">{ "TECH" }</option>
                            <option value="SUPER">{ "SUPER" }</option>
                            <option value="ADMIN">{ "ADMIN" }</option>
                        </select>
                    </label>
                    <LoadingButton label="Run" loading={*loading} onclick={run_query} />
                    <LoadingButton
                        label="Export CSV"
                        loading={*exporting}
                        onclick={export_csv}
                        kind={Some("secondary".to_string())}
                    />
                </div>
            </div>

            <table class="data">
                <thead>
                    <tr>
                        <th>{ "User" }</th><th>{ "Role" }</th>
                        <th>{ "Quiz avg" }</th><th>{ "Time (s)" }</th>
                        <th>{ "Completions" }</th><th>{ "Reviews" }</th>
                    </tr>
                </thead>
                <tbody>
                    { for rows.iter().map(|r| html!{
                        <tr>
                            <td>{ &r.username }</td>
                            <td>{ r.role.short() }</td>
                            <td>{ r.quiz_avg.map(|v| format!("{:.2}", v)).unwrap_or_default() }</td>
                            <td>{ r.time_spent_total.unwrap_or(0) }</td>
                            <td>{ r.completion_count.unwrap_or(0) }</td>
                            <td>{ r.review_total.unwrap_or(0) }</td>
                        </tr>
                    }) }
                    if rows.is_empty() && !*loading {
                        <tr><td colspan="6" class="muted" style="text-align:center; padding:24px;">
                            { "No data — press Run to load." }
                        </td></tr>
                    }
                </tbody>
            </table>
        </div>
    }
}

fn text_input(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
            state.set(el.value());
        }
    })
}

fn trigger_download(bytes: &[u8], filename: &str) -> Result<(), String> {
    let u8_array = js_sys::Uint8Array::from(bytes);
    let blob_parts = js_sys::Array::new();
    blob_parts.push(&u8_array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/csv");
    let blob = Blob::new_with_u8_array_sequence_and_options(&blob_parts, &opts)
        .map_err(|_| "blob construct")?;
    let url = Url::create_object_url_with_blob(&blob).map_err(|_| "url create")?;
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no doc")?;
    let a: HtmlAnchorElement = doc
        .create_element("a")
        .map_err(|_| "create element")?
        .dyn_into()
        .map_err(|_| "cast anchor")?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    let _ = Url::revoke_object_url(&url);
    Ok(())
}
