use wasm_bindgen::JsCast;
use web_sys::{Blob, HtmlAnchorElement, HtmlInputElement, HtmlSelectElement, Url};
use yew::prelude::*;

use crate::api;
use crate::app::{toast_err, toast_ok, AuthCtx, RoleGate, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::types::{Branch, DataEnvelope, LearningRow, Paginated, Role};

/// Lightweight UUID syntactic validator — good enough to fail fast on a
/// misspelled branch ID before the request hits the server. Accepts the
/// canonical 8-4-4-4-12 hex form, case-insensitive. Empty strings pass so
/// "no filter" doesn't trip the guard.
fn is_valid_uuid(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return true;
    }
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        let expect_hyphen = i == 8 || i == 13 || i == 18 || i == 23;
        let is_hex = matches!(*b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F');
        let ok = if expect_hyphen { *b == b'-' } else { is_hex };
        if !ok {
            return false;
        }
    }
    true
}

#[function_component(AnalyticsPage)]
pub fn analytics_page() -> Html {
    // Every authenticated role can reach this page: TECH sees their own
    // results (backend scopes the query by the caller's user_id), SUPER sees
    // their branch, ADMIN sees everything. Gating TECH out at the UI layer
    // violates the prompt — the backend already enforces scope.
    html! {
        <RoleGate allowed={vec![Role::Tech, Role::Super, Role::Admin]} fallback={html!{
            <div class="error-banner">{ "Sign in to see analytics." }</div>
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
    let branch = use_state(String::new);
    let rows = use_state(Vec::<LearningRow>::new);
    let loading = use_state(|| false);
    let exporting = use_state(|| false);
    // Dropdown source for the branch filter. Admins/supervisors can hit
    // `/api/admin/branches`; everyone else falls back to the text input with
    // UUID validation.
    let branches = use_state(Vec::<Branch>::new);
    let caller_role = auth
        .state
        .user
        .as_ref()
        .map(|u| u.role.clone());
    let can_list_branches = matches!(caller_role, Some(Role::Admin) | Some(Role::Super));

    {
        let branches = branches.clone();
        let auth = auth.clone();
        let enabled = can_list_branches;
        use_effect_with((), move |_| {
            if enabled {
                let state = auth.state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(p) = api::get::<Paginated<Branch>>(
                        "/api/admin/branches?per_page=200",
                        &state,
                    )
                    .await
                    {
                        branches.set(p.data);
                    }
                });
            }
            || ()
        });
    }

    let build_query = {
        let from = from.clone();
        let to = to.clone();
        let role = role.clone();
        let branch = branch.clone();
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
            let b = branch.trim();
            if !b.is_empty() {
                q.push_str(&format!("branch={}&", b));
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
        let branch = branch.clone();
        Callback::from(move |_| {
            if *loading {
                return;
            }
            if !is_valid_uuid(&branch) {
                toast_err(&toasts, "Branch filter must be a valid UUID");
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
        let branch = branch.clone();
        Callback::from(move |_| {
            if *exporting {
                return;
            }
            if !is_valid_uuid(&branch) {
                toast_err(&toasts, "Branch filter must be a valid UUID");
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
    let on_branch_text = text_input(branch.clone());
    let on_branch_select = {
        let branch = branch.clone();
        Callback::from(move |e: Event| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
                branch.set(el.value());
            }
        })
    };
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
                    // Branch filter: privileged roles get a populated selector
                    // from /api/admin/branches; everyone else keeps the raw
                    // UUID field with client-side format validation. Both
                    // paths feed the same `branch` state.
                    if !branches.is_empty() {
                        <label>{ "Branch" }
                            <select value={(*branch).clone()} onchange={on_branch_select}>
                                <option value="">{ "Any" }</option>
                                { for branches.iter().map(|b| html!{
                                    <option value={b.id.to_string()}>{ &b.name }</option>
                                }) }
                            </select>
                        </label>
                    } else {
                        <label>{ "Branch (UUID)" }
                            <input type="text" value={(*branch).clone()} oninput={on_branch_text}
                                   placeholder="optional branch id" />
                        </label>
                    }
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

            <TrendsPanel from={(*from).clone()} to={(*to).clone()} branch={(*branch).clone()} />
        </div>
    }
}

// ---------------------------------------------------------------------------
// Trends view: groups learning records by knowledge point / learning unit /
// workflow, with optional time bucketing. Backed by the new
// `/api/analytics/trends/*` endpoints. Exposes the `completion_rate` metric
// the PRD requires (in addition to raw completion counts).
// ---------------------------------------------------------------------------
#[derive(Clone, PartialEq, Properties)]
struct TrendsProps {
    from: String,
    to: String,
    branch: String,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
struct TrendRow {
    group_id: Option<uuid::Uuid>,
    group_label: Option<String>,
    bucket_start: Option<chrono::DateTime<chrono::Utc>>,
    attempt_count: Option<i64>,
    completion_count: Option<i64>,
    completion_rate: Option<f64>,
    avg_quiz_score: Option<f64>,
    avg_time_spent_seconds: Option<f64>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
struct TrendResp {
    data: Vec<TrendRow>,
}

#[function_component(TrendsPanel)]
fn trends_panel(props: &TrendsProps) -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let group_mode = use_state(|| "knowledge-points".to_string());
    let bucket = use_state(String::new); // "" | day | week | month
    let rows = use_state(Vec::<TrendRow>::new);
    let loading = use_state(|| false);

    let on_group = {
        let group_mode = group_mode.clone();
        Callback::from(move |e: Event| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
                group_mode.set(el.value());
            }
        })
    };
    let on_bucket = {
        let bucket = bucket.clone();
        Callback::from(move |e: Event| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
                bucket.set(el.value());
            }
        })
    };

    let run = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let rows = rows.clone();
        let loading = loading.clone();
        let group_mode = group_mode.clone();
        let bucket = bucket.clone();
        let from = props.from.clone();
        let to = props.to.clone();
        let branch = props.branch.clone();
        Callback::from(move |_| {
            if *loading {
                return;
            }
            loading.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let rows = rows.clone();
            let loading = loading.clone();
            let mode = (*group_mode).clone();
            let bucket_val = (*bucket).clone();
            let mut q = String::new();
            if !from.is_empty() {
                q.push_str(&format!("from={}&", from));
            }
            if !to.is_empty() {
                q.push_str(&format!("to={}&", to));
            }
            let b = branch.trim();
            if !b.is_empty() {
                q.push_str(&format!("branch={}&", b));
            }
            if !bucket_val.is_empty() {
                q.push_str(&format!("bucket={}&", bucket_val));
            }
            if q.ends_with('&') {
                q.pop();
            }
            let url = if q.is_empty() {
                format!("/api/analytics/trends/{}", mode)
            } else {
                format!("/api/analytics/trends/{}?{}", mode, q)
            };
            wasm_bindgen_futures::spawn_local(async move {
                match api::get::<TrendResp>(&url, &state).await {
                    Ok(r) => rows.set(r.data),
                    Err(e) => toast_err(&toasts, format!("Trend query failed: {}", e.message)),
                }
                loading.set(false);
            });
        })
    };

    let group_label = match group_mode.as_str() {
        "units" => "Learning unit",
        "workflows" => "Workflow",
        _ => "Knowledge point",
    };

    html! {
        <div class="card">
            <h3>{ "Trends" }</h3>
            <div class="row">
                <label>{ "Group by" }
                    <select value={(*group_mode).clone()} onchange={on_group}>
                        <option value="knowledge-points">{ "Knowledge point" }</option>
                        <option value="units">{ "Learning unit (recipe)" }</option>
                        <option value="workflows">{ "Workflow (work order)" }</option>
                    </select>
                </label>
                <label>{ "Time bucket" }
                    <select value={(*bucket).clone()} onchange={on_bucket}>
                        <option value="">{ "None" }</option>
                        <option value="day">{ "Day" }</option>
                        <option value="week">{ "Week" }</option>
                        <option value="month">{ "Month" }</option>
                    </select>
                </label>
                <LoadingButton label="Run trends" loading={*loading} onclick={run}
                    kind={Some("secondary".to_string())} />
            </div>

            <table class="data">
                <thead>
                    <tr>
                        <th>{ group_label }</th>
                        <th>{ "Bucket" }</th>
                        <th>{ "Attempts" }</th>
                        <th>{ "Completions" }</th>
                        <th>{ "Completion rate" }</th>
                        <th>{ "Quiz avg" }</th>
                        <th>{ "Avg time (s)" }</th>
                    </tr>
                </thead>
                <tbody>
                    { for rows.iter().map(|r| html!{
                        <tr>
                            <td>{ r.group_label.clone().unwrap_or_else(|| "—".into()) }</td>
                            <td>{ r.bucket_start.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| "—".into()) }</td>
                            <td>{ r.attempt_count.unwrap_or(0) }</td>
                            <td>{ r.completion_count.unwrap_or(0) }</td>
                            <td>{ r.completion_rate.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "—".into()) }</td>
                            <td>{ r.avg_quiz_score.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".into()) }</td>
                            <td>{ r.avg_time_spent_seconds.map(|v| format!("{:.0}", v)).unwrap_or_else(|| "—".into()) }</td>
                        </tr>
                    }) }
                    if rows.is_empty() && !*loading {
                        <tr><td colspan="7" class="muted" style="text-align:center; padding:24px;">
                            { "No trend data — press Run trends." }
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

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn is_valid_uuid_accepts_canonical_hex_form() {
        assert!(is_valid_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(is_valid_uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
        // Mixed case is allowed — the backend normalizes.
        assert!(is_valid_uuid("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"));
    }

    #[wasm_bindgen_test]
    fn is_valid_uuid_treats_empty_as_ok() {
        // Empty string means "no filter" — the builder must not escalate.
        assert!(is_valid_uuid(""));
        assert!(is_valid_uuid("   "));
    }

    #[wasm_bindgen_test]
    fn is_valid_uuid_rejects_malformed_strings() {
        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(!is_valid_uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeeZ"), "non-hex char");
        assert!(!is_valid_uuid("aaaaaaaabbbbccccddddeeeeeeeeeeee"), "missing hyphens");
        assert!(
            !is_valid_uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee-extra"),
            "length > 36"
        );
        assert!(!is_valid_uuid("aaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"), "length < 36");
    }
}
