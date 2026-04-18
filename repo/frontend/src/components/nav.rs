use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::{AuthAction, AuthCtx};
use crate::routes::Route;
use crate::types::Role;

#[function_component(TopBar)]
pub fn top_bar() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let loc = use_location();
    let cur = loc.as_ref().map(|l| l.path().to_string()).unwrap_or_default();
    let user = auth.state.user.clone();
    let role = user.as_ref().map(|u| u.role.clone());

    let on_logout = {
        let auth = auth.clone();
        Callback::from(move |_| auth.dispatch(AuthAction::Logout))
    };

    let link = |route: Route, label: &str, path: &str| -> Html {
        let active = if path == "/" {
            cur == "/" || cur == "/dashboard"
        } else {
            cur.starts_with(path)
        };
        let class = if active { "active" } else { "" };
        html! { <Link<Route> to={route} classes={class}>{ label }</Link<Route>> }
    };

    html! {
        <nav class="topbar">
            <span class="brand">{ "FieldOps" }</span>
            { link(Route::Dashboard, "Jobs", "/dashboard") }
            { link(Route::Notifications, "Notifications", "/notifications") }
            if role == Some(Role::Super) || role == Some(Role::Admin) {
                { link(Route::Analytics, "Analytics", "/analytics") }
            }
            if role == Some(Role::Admin) {
                { link(Route::Admin, "Admin", "/admin") }
            }
            <span class="spacer"></span>
            if let Some(u) = &user {
                <span class="who">{ format!("{} · {}", u.username, u.role.short()) }</span>
            }
            <button class="ghost" onclick={on_logout}>{ "Sign out" }</button>
        </nav>
    }
}
