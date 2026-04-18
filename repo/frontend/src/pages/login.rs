use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api;
use crate::app::{toast_err, toast_ok, AuthAction, AuthCtx, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::routes::Route;
use crate::types::LoginResponse;

#[function_component(LoginPage)]
pub fn login_page() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");
    let nav = use_navigator().expect("navigator");

    let username = use_state(String::new);
    let password = use_state(String::new);
    let loading = use_state(|| false);

    let on_user = {
        let username = username.clone();
        Callback::from(move |e: InputEvent| {
            let t = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            if let Some(el) = t {
                username.set(el.value());
            }
        })
    };
    let on_pass = {
        let password = password.clone();
        Callback::from(move |e: InputEvent| {
            let t = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            if let Some(el) = t {
                password.set(el.value());
            }
        })
    };

    let submit = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let nav = nav.clone();
        let username = username.clone();
        let password = password.clone();
        let loading = loading.clone();
        Callback::from(move |_| {
            if *loading {
                return;
            }
            let u = (*username).clone();
            let p = (*password).clone();
            if u.is_empty() || p.is_empty() {
                toast_err(&toasts, "Username and password are required.");
                return;
            }
            loading.set(true);
            let state = auth.clone();
            let toasts = toasts.clone();
            let nav = nav.clone();
            let loading = loading.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({ "username": u, "password": p });
                match api::post::<_, LoginResponse>("/api/auth/login", &body, &state.state).await {
                    Ok(resp) => {
                        state.dispatch(AuthAction::Login(resp.token.clone(), resp.user.clone()));
                        toast_ok(&toasts, format!("Welcome, {}", resp.user.username));
                        nav.push(&Route::Dashboard);
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                loading.set(false);
            });
        })
    };

    let on_key = {
        let submit = submit.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                submit.emit(MouseEvent::new("click").unwrap());
            }
        })
    };

    html! {
        <div class="login-wrap">
            <div class="card login-card">
                <h1>{ "FieldOps" }</h1>
                <p class="muted">{ "Kitchen & Training Console" }</p>
                <div class="stack" onkeyup={on_key}>
                    <label>
                        { "Username" }
                        <input
                            type="text"
                            value={(*username).clone()}
                            oninput={on_user}
                            autocomplete="username"
                            disabled={*loading}
                        />
                    </label>
                    <label>
                        { "Password" }
                        <input
                            type="password"
                            value={(*password).clone()}
                            oninput={on_pass}
                            autocomplete="current-password"
                            disabled={*loading}
                        />
                    </label>
                    <LoadingButton
                        label={ if *loading { "Signing in...".to_string() } else { "Sign in".to_string() } }
                        loading={*loading}
                        onclick={submit}
                    />
                    <p class="small muted">
                        { "Default admin: " }<code>{ "admin" }</code>{ " / " }<code>{ "admin123" }</code>
                    </p>
                </div>
            </div>
        </div>
    }
}
