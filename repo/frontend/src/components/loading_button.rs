//! Button that shows a spinner + disables itself while a prop-controlled
//! `loading` flag is true. Enforces the PRD rule: every action button exposes
//! loading/disabled states.

use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct LoadingButtonProps {
    pub label: String,
    pub onclick: Callback<MouseEvent>,
    #[prop_or_default]
    pub loading: bool,
    #[prop_or_default]
    pub disabled: bool,
    #[prop_or_default]
    pub class: Option<String>,
    #[prop_or_default]
    pub kind: Option<String>, // primary | secondary | danger | ghost
}

#[function_component(LoadingButton)]
pub fn loading_button(props: &LoadingButtonProps) -> Html {
    let mut cls = match props.kind.as_deref() {
        Some("secondary") => String::from("secondary"),
        Some("danger") => String::from("danger"),
        Some("ghost") => String::from("ghost"),
        _ => String::new(),
    };
    if let Some(extra) = &props.class {
        if !cls.is_empty() {
            cls.push(' ');
        }
        cls.push_str(extra);
    }
    let disabled = props.disabled || props.loading;
    html! {
        <button
            class={cls}
            disabled={disabled}
            onclick={props.onclick.clone()}
        >
            if props.loading { <span class="spinner" aria-hidden="true"></span> }
            <span>{ &props.label }</span>
        </button>
    }
}
