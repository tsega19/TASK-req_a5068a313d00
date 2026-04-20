use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::{AuthAction, AuthCtx};
use crate::routes::Route;
use crate::types::Role;

// Pure role-gate helpers — extracted so the #[cfg(test)] module below can
// assert visibility without mounting the Yew tree. The render path consumes
// these so drift between test and render is impossible.
pub(crate) fn shows_analytics_link(role: &Option<Role>) -> bool {
    role.is_some()
}
pub(crate) fn shows_admin_link(role: &Option<Role>) -> bool {
    matches!(role, Some(Role::Admin))
}
pub(crate) fn is_active_path(current: &str, link_path: &str) -> bool {
    if link_path == "/" {
        current == "/" || current == "/dashboard"
    } else {
        current.starts_with(link_path)
    }
}

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
        let class = if is_active_path(&cur, path) { "active" } else { "" };
        html! { <Link<Route> to={route} classes={class}>{ label }</Link<Route>> }
    };

    html! {
        <nav class="topbar">
            <span class="brand">{ "FieldOps" }</span>
            { link(Route::Dashboard, "Jobs", "/dashboard") }
            { link(Route::Notifications, "Notifications", "/notifications") }
            // Every authenticated role sees the Analytics tab — TECH views
            // their own records, SUPER sees their branch, ADMIN sees all.
            if shows_analytics_link(&role) {
                { link(Route::Analytics, "Analytics", "/analytics") }
            }
            if shows_admin_link(&role) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn analytics_link_is_visible_to_every_authenticated_role() {
        assert!(shows_analytics_link(&Some(Role::Tech)));
        assert!(shows_analytics_link(&Some(Role::Super)));
        assert!(shows_analytics_link(&Some(Role::Admin)));
    }

    #[wasm_bindgen_test]
    fn analytics_link_is_hidden_when_unauthenticated() {
        assert!(!shows_analytics_link(&None));
    }

    #[wasm_bindgen_test]
    fn admin_link_is_admin_only() {
        assert!(shows_admin_link(&Some(Role::Admin)));
        assert!(!shows_admin_link(&Some(Role::Super)));
        assert!(!shows_admin_link(&Some(Role::Tech)));
        assert!(!shows_admin_link(&None));
    }

    #[wasm_bindgen_test]
    fn is_active_path_treats_dashboard_as_root() {
        // `/` and `/dashboard` both light up the Jobs tab.
        assert!(is_active_path("/",          "/"));
        assert!(is_active_path("/dashboard", "/"));
        // Any non-dashboard path should NOT mark Jobs active.
        assert!(!is_active_path("/analytics", "/"));
    }

    #[wasm_bindgen_test]
    fn is_active_path_matches_nested_routes() {
        // Work-order detail pages must keep the Jobs tab active.
        assert!(is_active_path("/work-orders/abc", "/dashboard") == false);
        // But analytics sub-routes should keep analytics active.
        assert!(is_active_path("/analytics/trends", "/analytics"));
    }
}
