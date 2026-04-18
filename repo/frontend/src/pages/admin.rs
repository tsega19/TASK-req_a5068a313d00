use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use crate::api;
use crate::app::{toast_err, toast_ok, AuthCtx, RoleGate, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::types::{Branch, Paginated, Role};

#[function_component(AdminPanel)]
pub fn admin_panel() -> Html {
    html! {
        <RoleGate allowed={vec![Role::Admin]} fallback={html!{
            <div class="error-banner">{ "Admin panel — ADMIN role required." }</div>
        }}>
            <div class="stack">
                <h1>{ "Admin panel" }</h1>
                <UsersSection />
                <BranchesSection />
                <TipCardsSection />
                <SyncSection />
            </div>
        </RoleGate>
    }
}

// -----------------------------------------------------------------------------
// Users
// -----------------------------------------------------------------------------
#[derive(Clone, PartialEq, serde::Deserialize)]
struct AdminUser {
    id: uuid::Uuid,
    username: String,
    role: Role,
    full_name: Option<String>,
    branch_id: Option<uuid::Uuid>,
    privacy_mode: bool,
}

#[function_component(UsersSection)]
fn users_section() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let users = use_state(Vec::<AdminUser>::new);
    let loading = use_state(|| true);
    let reload = use_state(|| 0u32);

    let username = use_state(String::new);
    let password = use_state(String::new);
    let role = use_state(|| "TECH".to_string());
    let creating = use_state(|| false);

    {
        let users = users.clone();
        let loading = loading.clone();
        let auth = auth.clone();
        let toasts = toasts.clone();
        let dep = *reload;
        use_effect_with(dep, move |_| {
            loading.set(true);
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::get::<Paginated<AdminUser>>("/api/admin/users?per_page=200", &state).await {
                    Ok(p) => users.set(p.data),
                    Err(e) => toast_err(&toasts, format!("Users: {}", e.message)),
                }
                loading.set(false);
            });
            || ()
        });
    }

    let create_user = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let reload = reload.clone();
        let creating = creating.clone();
        let username = username.clone();
        let password = password.clone();
        let role = role.clone();
        Callback::from(move |_| {
            if *creating {
                return;
            }
            if username.is_empty() || password.len() < 4 {
                toast_err(&toasts, "Username + password (≥4 chars) required");
                return;
            }
            creating.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let reload = reload.clone();
            let creating = creating.clone();
            let username_val = (*username).clone();
            let password_val = (*password).clone();
            let role_val = (*role).clone();
            let username = username.clone();
            let password = password.clone();
            let reload_val = *reload;
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({
                    "username": username_val,
                    "password": password_val,
                    "role": role_val,
                });
                match api::post::<_, serde_json::Value>("/api/admin/users", &body, &state).await {
                    Ok(_) => {
                        toast_ok(&toasts, "User created");
                        username.set(String::new());
                        password.set(String::new());
                        reload.set(reload_val + 1);
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                creating.set(false);
            });
        })
    };

    let delete_user = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let reload = reload.clone();
        Callback::from(move |id: uuid::Uuid| {
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let reload = reload.clone();
            let reload_val = *reload;
            wasm_bindgen_futures::spawn_local(async move {
                match api::delete_(&format!("/api/admin/users/{}", id), &state).await {
                    Ok(_) => {
                        toast_ok(&toasts, "User removed");
                        reload.set(reload_val + 1);
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
            });
        })
    };

    let on_user = text_input(username.clone());
    let on_pass = text_input(password.clone());
    let on_role = select_input(role.clone());

    html! {
        <div class="card">
            <h3>{ "Users" }</h3>
            <div class="row">
                <label>{ "Username" }<input value={(*username).clone()} oninput={on_user} /></label>
                <label>{ "Password" }<input type="password" value={(*password).clone()} oninput={on_pass} /></label>
                <label>{ "Role" }
                    <select value={(*role).clone()} onchange={on_role}>
                        <option value="TECH">{ "TECH" }</option>
                        <option value="SUPER">{ "SUPER" }</option>
                        <option value="ADMIN">{ "ADMIN" }</option>
                    </select>
                </label>
                <LoadingButton label="Create user" loading={*creating} onclick={create_user} />
            </div>

            if *loading {
                <p class="muted">{ "Loading users..." }</p>
            } else {
                <table class="data">
                    <thead>
                        <tr><th>{"Username"}</th><th>{"Role"}</th><th>{"Privacy"}</th><th></th></tr>
                    </thead>
                    <tbody>
                        { for users.iter().map(|u| {
                            let cb = delete_user.clone();
                            let id = u.id;
                            let onclick = Callback::from(move |_| cb.emit(id));
                            html! {
                                <tr>
                                    <td>{ &u.username }</td>
                                    <td>{ u.role.short() }</td>
                                    <td>{ if u.privacy_mode { "ON" } else { "OFF" } }</td>
                                    <td><button class="danger" onclick={onclick}>{ "Delete" }</button></td>
                                </tr>
                            }
                        }) }
                    </tbody>
                </table>
            }
        </div>
    }
}

// -----------------------------------------------------------------------------
// Branches
// -----------------------------------------------------------------------------
#[function_component(BranchesSection)]
fn branches_section() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let branches = use_state(Vec::<Branch>::new);
    let reload = use_state(|| 0u32);
    let creating = use_state(|| false);

    let name = use_state(String::new);
    let radius = use_state(|| "30".to_string());

    {
        let branches = branches.clone();
        let auth = auth.clone();
        let toasts = toasts.clone();
        let dep = *reload;
        use_effect_with(dep, move |_| {
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::get::<Paginated<Branch>>("/api/admin/branches?per_page=100", &state).await {
                    Ok(p) => branches.set(p.data),
                    Err(e) => toast_err(&toasts, format!("Branches: {}", e.message)),
                }
            });
            || ()
        });
    }

    let create_branch = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let reload = reload.clone();
        let creating = creating.clone();
        let name = name.clone();
        let radius = radius.clone();
        Callback::from(move |_| {
            if *creating {
                return;
            }
            creating.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let reload = reload.clone();
            let creating = creating.clone();
            let name_val = (*name).clone();
            let radius_val: i32 = (*radius).parse().unwrap_or(30);
            let name = name.clone();
            let reload_val = *reload;
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({
                    "name": name_val,
                    "service_radius_miles": radius_val,
                });
                match api::post::<_, serde_json::Value>("/api/admin/branches", &body, &state).await {
                    Ok(_) => {
                        toast_ok(&toasts, "Branch created");
                        name.set(String::new());
                        reload.set(reload_val + 1);
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                creating.set(false);
            });
        })
    };

    let on_name = text_input(name.clone());
    let on_radius = text_input(radius.clone());

    html! {
        <div class="card">
            <h3>{ "Branches" }</h3>
            <div class="row">
                <label>{ "Name" }<input value={(*name).clone()} oninput={on_name} /></label>
                <label>{ "Service radius (mi)" }<input type="number" value={(*radius).clone()} oninput={on_radius} /></label>
                <LoadingButton label="Create branch" loading={*creating} onclick={create_branch} />
            </div>
            <table class="data">
                <thead><tr><th>{"Name"}</th><th>{"Radius"}</th><th>{"Lat,Lng"}</th></tr></thead>
                <tbody>
                    { for branches.iter().map(|b| html!{
                        <tr>
                            <td>{ &b.name }</td>
                            <td>{ b.service_radius_miles }</td>
                            <td>{ format!("{},{}",
                                b.lat.map(|v| format!("{:.3}", v)).unwrap_or("—".into()),
                                b.lng.map(|v| format!("{:.3}", v)).unwrap_or("—".into())) }</td>
                        </tr>
                    }) }
                </tbody>
            </table>
        </div>
    }
}

// -----------------------------------------------------------------------------
// Tip card authoring
// -----------------------------------------------------------------------------
#[function_component(TipCardsSection)]
fn tip_cards_section() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");

    let step_id = use_state(String::new);
    let title = use_state(String::new);
    let content = use_state(String::new);
    let pinned = use_state(|| true);
    let saving = use_state(|| false);

    let on_step = text_input(step_id.clone());
    let on_title = text_input(title.clone());
    let on_content = {
        let content = content.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok()) {
                content.set(el.value());
            }
        })
    };
    let on_pinned = {
        let pinned = pinned.clone();
        Callback::from(move |_| pinned.set(!*pinned))
    };

    let save = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let saving = saving.clone();
        let step_id = step_id.clone();
        let title = title.clone();
        let content = content.clone();
        let pinned = pinned.clone();
        Callback::from(move |_| {
            if *saving {
                return;
            }
            let step = (*step_id).trim().to_string();
            let t = (*title).trim().to_string();
            let c = (*content).trim().to_string();
            if step.is_empty() || t.is_empty() || c.is_empty() {
                toast_err(&toasts, "step_id, title, content are required");
                return;
            }
            let step_uuid: uuid::Uuid = match step.parse() {
                Ok(u) => u,
                Err(_) => {
                    toast_err(&toasts, "step_id must be a UUID");
                    return;
                }
            };
            saving.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let saving = saving.clone();
            let is_pinned = *pinned;
            let title = title.clone();
            let content = content.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({
                    "step_id": step_uuid,
                    "title": t,
                    "content": c,
                    "is_pinned": is_pinned,
                });
                match api::post::<_, serde_json::Value>("/api/tip-cards", &body, &state).await {
                    Ok(_) => {
                        toast_ok(&toasts, "Tip card saved");
                        title.set(String::new());
                        content.set(String::new());
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                saving.set(false);
            });
        })
    };

    html! {
        <div class="card">
            <h3>{ "Author tip card" }</h3>
            <div class="stack">
                <label>{ "Step ID (UUID)" }<input value={(*step_id).clone()} oninput={on_step} /></label>
                <label>{ "Title" }<input value={(*title).clone()} oninput={on_title} /></label>
                <label>{ "Content" }<textarea value={(*content).clone()} oninput={on_content} /></label>
                <label class="switch">
                    <input type="checkbox" checked={*pinned} onchange={on_pinned} />
                    <span class="track"></span>
                    <span>{ if *pinned { "Pinned" } else { "Not pinned" } }</span>
                </label>
                <LoadingButton label="Save tip card" loading={*saving} onclick={save} />
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// Sync trigger
// -----------------------------------------------------------------------------
#[function_component(SyncSection)]
fn sync_section() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let running = use_state(|| false);
    let last = use_state(|| None::<serde_json::Value>);

    let trigger = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let running = running.clone();
        let last = last.clone();
        Callback::from(move |_| {
            if *running {
                return;
            }
            running.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let running = running.clone();
            let last = last.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::post::<_, serde_json::Value>(
                    "/api/admin/sync/trigger",
                    &serde_json::json!({}),
                    &state,
                )
                .await
                {
                    Ok(v) => {
                        toast_ok(&toasts, "Sync run complete");
                        last.set(Some(v));
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                running.set(false);
            });
        })
    };

    html! {
        <div class="card">
            <h3>{ "Sync" }</h3>
            <p class="muted small">
                { "Scheduled every " }{ "10 min" }{ " via the backend ticker. Manual trigger runs it immediately." }
            </p>
            <LoadingButton label="Run sync now" loading={*running} onclick={trigger} />
            if let Some(r) = last.as_ref() {
                <pre style="background:#0f172a; padding:12px; border-radius:8px; overflow:auto; font-size:13px;">
                    { r.to_string() }
                </pre>
            }
        </div>
    }
}

// -----------------------------------------------------------------------------
fn text_input(state: UseStateHandle<String>) -> Callback<InputEvent> {
    Callback::from(move |e: InputEvent| {
        if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
            state.set(el.value());
        }
    })
}

fn select_input(state: UseStateHandle<String>) -> Callback<Event> {
    Callback::from(move |e: Event| {
        if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
            state.set(el.value());
        }
    })
}
