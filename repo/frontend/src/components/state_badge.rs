use yew::prelude::*;

use crate::types::{Priority, WorkOrderState};

#[derive(Properties, PartialEq)]
pub struct StateBadgeProps {
    pub state: WorkOrderState,
}

#[function_component(StateBadge)]
pub fn state_badge(props: &StateBadgeProps) -> Html {
    let css = format!("badge state-{}", props.state.css_key());
    html! { <span class={css}>{ props.state.label() }</span> }
}

#[derive(Properties, PartialEq)]
pub struct PriorityBadgeProps {
    pub priority: Priority,
}

#[function_component(PriorityBadge)]
pub fn priority_badge(props: &PriorityBadgeProps) -> Html {
    let css = format!("badge priority-{}", props.priority.label());
    html! { <span class={css}>{ props.priority.label() }</span> }
}
