//! Root component. Provides auth + toast contexts and the BrowserRouter.

use std::rc::Rc;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::auth::AuthState;
use crate::components::nav::TopBar;
use crate::components::toast::{Toast, ToastKind, ToastStack};
use crate::pages;
use crate::routes::Route;
use crate::types::Role;

// -----------------------------------------------------------------------------
// Context: AuthCtx (token + user, mutable via reducer)
// -----------------------------------------------------------------------------
pub enum AuthAction {
    Login(String, crate::types::User),
    Logout,
    Refresh,
}

#[derive(Clone, PartialEq)]
pub struct AuthCtxInner {
    pub state: AuthState,
}

impl Reducible for AuthCtxInner {
    type Action = AuthAction;
    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        let state = match action {
            AuthAction::Login(token, user) => AuthState::save(&token, &user),
            AuthAction::Logout => AuthState::clear(),
            AuthAction::Refresh => AuthState::load(),
        };
        Rc::new(AuthCtxInner { state })
    }
}

pub type AuthCtx = UseReducerHandle<AuthCtxInner>;

// -----------------------------------------------------------------------------
// Context: ToastCtx (append-only queue of transient banners)
// -----------------------------------------------------------------------------
pub enum ToastAction {
    Push(Toast),
    Dismiss(u64),
}

#[derive(Clone, PartialEq, Default)]
pub struct ToastCtxInner {
    pub toasts: Vec<Toast>,
    pub next_id: u64,
}

impl Reducible for ToastCtxInner {
    type Action = ToastAction;
    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        match action {
            ToastAction::Push(mut t) => {
                let mut next = (*self).clone();
                t.id = next.next_id;
                next.next_id += 1;
                next.toasts.push(t);
                Rc::new(next)
            }
            ToastAction::Dismiss(id) => {
                let mut next = (*self).clone();
                next.toasts.retain(|t| t.id != id);
                Rc::new(next)
            }
        }
    }
}

pub type ToastCtx = UseReducerHandle<ToastCtxInner>;

// Convenience helpers to push toasts from anywhere with a ToastCtx handle.
pub fn toast_err(ctx: &ToastCtx, msg: impl Into<String>) {
    ctx.dispatch(ToastAction::Push(Toast::new(ToastKind::Err, msg.into())));
}
pub fn toast_ok(ctx: &ToastCtx, msg: impl Into<String>) {
    ctx.dispatch(ToastAction::Push(Toast::new(ToastKind::Ok, msg.into())));
}
pub fn toast_warn(ctx: &ToastCtx, msg: impl Into<String>) {
    ctx.dispatch(ToastAction::Push(Toast::new(ToastKind::Warn, msg.into())));
}

// -----------------------------------------------------------------------------
// App
// -----------------------------------------------------------------------------
#[function_component(App)]
pub fn app() -> Html {
    let auth = use_reducer(|| AuthCtxInner { state: AuthState::load() });
    let toasts = use_reducer(ToastCtxInner::default);

    html! {
        <ContextProvider<AuthCtx> context={auth.clone()}>
            <ContextProvider<ToastCtx> context={toasts.clone()}>
                <BrowserRouter>
                    <Shell />
                </BrowserRouter>
            </ContextProvider<ToastCtx>>
        </ContextProvider<AuthCtx>>
    }
}

#[function_component(Shell)]
fn shell() -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let nav = use_navigator().expect("navigator");

    // Route-gate: send unauthenticated users to /login unless they're already there.
    {
        let loc = use_location();
        let nav = nav.clone();
        let authed = auth.state.is_authed();
        use_effect_with(
            (authed, loc.as_ref().map(|l| l.path().to_string()).unwrap_or_default()),
            move |(authed, path)| {
                if !*authed && path != "/login" {
                    nav.push(&Route::Login);
                } else if *authed && path == "/login" {
                    nav.push(&Route::Dashboard);
                }
                || ()
            },
        );
    }

    // Offline-first sync loop: once the user is authenticated, start a
    // background Interval that pulls `/api/sync/changes` and replays any
    // queued mutations. The Interval is kept alive via `use_state` —
    // dropping it on logout stops the loop automatically.
    {
        let authed = auth.state.is_authed();
        let state = auth.state.clone();
        use_effect_with(authed, move |authed| {
            let handle: Option<gloo_timers::callback::Interval> = if *authed {
                Some(crate::offline::start_sync_loop(state))
            } else {
                None
            };
            move || drop(handle)
        });
    }

    html! {
        <div class="app-shell">
            if auth.state.is_authed() { <TopBar /> }
            <main>
                <Switch<Route> render={switch} />
            </main>
            <ToastStack />
        </div>
    }
}

fn switch(route: Route) -> Html {
    match route {
        Route::Home | Route::Dashboard => html! { <pages::dashboard::Dashboard /> },
        Route::Login => html! { <pages::login::LoginPage /> },
        Route::WorkOrder { id } => html! { <pages::work_order_detail::WorkOrderDetail {id} /> },
        Route::RecipeStep { id, step_id } => {
            html! { <pages::recipe_step::RecipeStepPage work_order_id={id} step_id={step_id} /> }
        }
        Route::MapView { id } => html! { <pages::map_view::MapViewPage {id} /> },
        Route::Notifications => html! { <pages::notifications::NotificationsPage /> },
        Route::Analytics => html! { <pages::analytics::AnalyticsPage /> },
        Route::Admin => html! { <pages::admin::AdminPanel /> },
        Route::NotFound => html! {
            <div class="empty-state">
                <h2>{ "Page not found" }</h2>
                <Link<Route> to={Route::Dashboard}><button>{ "Back to dashboard" }</button></Link<Route>>
            </div>
        },
    }
}

/// Show an element only if the authed user has one of the listed roles.
#[derive(Properties, PartialEq)]
pub struct RoleGateProps {
    pub allowed: Vec<Role>,
    pub children: Children,
    #[prop_or_default]
    pub fallback: Option<Html>,
}

#[function_component(RoleGate)]
pub fn role_gate(props: &RoleGateProps) -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let role = auth.state.user.as_ref().map(|u| u.role.clone());
    let allowed = role.map(|r| props.allowed.contains(&r)).unwrap_or(false);
    if allowed {
        html! { <>{ for props.children.iter() }</> }
    } else {
        props.fallback.clone().unwrap_or_default()
    }
}
