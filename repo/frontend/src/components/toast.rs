//! Transient toast notifications. Auto-dismiss after ~4 seconds.

use gloo_timers::callback::Timeout;
use yew::prelude::*;

use crate::app::{ToastAction, ToastCtx};

#[derive(Clone, PartialEq, Debug)]
pub enum ToastKind {
    Ok,
    Err,
    Warn,
}

impl ToastKind {
    // `pub(crate)` so the #[cfg(test)] module below can exercise the mapping
    // without rendering. The stylesheet uses these exact classes.
    pub(crate) fn css(&self) -> &'static str {
        match self {
            ToastKind::Ok => "toast ok",
            ToastKind::Err => "toast err",
            ToastKind::Warn => "toast warn",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
}

impl Toast {
    pub fn new(kind: ToastKind, message: String) -> Self {
        Self { id: 0, kind, message }
    }
}

#[function_component(ToastStack)]
pub fn toast_stack() -> Html {
    let ctx = use_context::<ToastCtx>().expect("toast ctx");
    html! {
        <div class="toast-stack" role="status" aria-live="polite">
            { for ctx.toasts.iter().map(|t| html!{ <ToastItem key={t.id} toast={t.clone()} /> }) }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ToastItemProps {
    pub toast: Toast,
}

#[function_component(ToastItem)]
fn toast_item(props: &ToastItemProps) -> Html {
    let ctx = use_context::<ToastCtx>().expect("toast ctx");
    {
        let ctx = ctx.clone();
        let id = props.toast.id;
        use_effect_with(id, move |id| {
            let id = *id;
            let timeout = Timeout::new(4000, move || {
                ctx.dispatch(ToastAction::Dismiss(id));
            });
            move || drop(timeout)
        });
    }
    let dismiss = {
        let ctx = ctx.clone();
        let id = props.toast.id;
        Callback::from(move |_| ctx.dispatch(ToastAction::Dismiss(id)))
    };
    html! {
        <div class={props.toast.kind.css()} onclick={dismiss}>
            { &props.toast.message }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn toast_kind_css_is_stable() {
        // The stylesheet relies on these exact kind-to-class mappings. A
        // rename here without a matching CSS update silently hides toasts.
        assert_eq!(ToastKind::Ok.css(),   "toast ok");
        assert_eq!(ToastKind::Err.css(),  "toast err");
        assert_eq!(ToastKind::Warn.css(), "toast warn");
    }

    #[wasm_bindgen_test]
    fn toast_new_preserves_kind_and_message() {
        let t = Toast::new(ToastKind::Err, "failed to save".to_string());
        assert_eq!(t.kind, ToastKind::Err);
        assert_eq!(t.message, "failed to save");
        // id is assigned by the reducer, `new` stubs 0.
        assert_eq!(t.id, 0);
    }
}
