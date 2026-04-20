use yew::prelude::*;

use crate::types::{Priority, WorkOrderState};

#[derive(Properties, PartialEq)]
pub struct StateBadgeProps {
    pub state: WorkOrderState,
}

#[function_component(StateBadge)]
pub fn state_badge(props: &StateBadgeProps) -> Html {
    let css = state_badge_css(&props.state);
    html! { <span class={css}>{ props.state.label() }</span> }
}

#[derive(Properties, PartialEq)]
pub struct PriorityBadgeProps {
    pub priority: Priority,
}

#[function_component(PriorityBadge)]
pub fn priority_badge(props: &PriorityBadgeProps) -> Html {
    let css = priority_badge_css(&props.priority);
    html! { <span class={css}>{ props.priority.label() }</span> }
}

// Exposed for tests: compute the css class string a badge uses so we can
// assert the stylesheet contract without rendering to a DOM.
pub(crate) fn state_badge_css(state: &WorkOrderState) -> String {
    format!("badge state-{}", state.css_key())
}
pub(crate) fn priority_badge_css(priority: &Priority) -> String {
    format!("badge priority-{}", priority.label())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn state_badge_css_matches_stylesheet_keys() {
        // Stylesheet (main.css) expects these exact class names. Drift here
        // silently turns badges grey, so pin the mapping.
        assert_eq!(state_badge_css(&WorkOrderState::Scheduled), "badge state-Scheduled");
        assert_eq!(state_badge_css(&WorkOrderState::EnRoute),   "badge state-EnRoute");
        assert_eq!(state_badge_css(&WorkOrderState::OnSite),    "badge state-OnSite");
        assert_eq!(state_badge_css(&WorkOrderState::Completed), "badge state-Completed");
        assert_eq!(state_badge_css(&WorkOrderState::Canceled),  "badge state-Canceled");
    }

    #[wasm_bindgen_test]
    fn priority_badge_css_matches_stylesheet_keys() {
        assert_eq!(priority_badge_css(&Priority::Low),      "badge priority-LOW");
        assert_eq!(priority_badge_css(&Priority::Normal),   "badge priority-NORMAL");
        assert_eq!(priority_badge_css(&Priority::High),     "badge priority-HIGH");
        assert_eq!(priority_badge_css(&Priority::Critical), "badge priority-CRITICAL");
    }

    // ---------------------------------------------------------------
    // Real render tests — mount the component into a real DOM node
    // via yew::Renderer, then query the resulting HTML. This gives us
    // a component-interaction test (not just a helper check) that
    // catches rename/reshape regressions in the render body itself.
    // ---------------------------------------------------------------

    async fn tick() {
        // Give Yew's scheduler a microtask to finish rendering. A short
        // real-time wait is more portable across the headless-Chromium
        // test runner than requestAnimationFrame.
        gloo_timers::future::TimeoutFuture::new(30).await;
    }

    fn mount_root() -> web_sys::Element {
        let doc = web_sys::window().unwrap().document().unwrap();
        let root = doc.create_element("div").unwrap();
        doc.body().unwrap().append_child(&root).unwrap();
        root
    }

    #[wasm_bindgen_test]
    async fn state_badge_renders_span_with_expected_class_and_label() {
        let root = mount_root();
        yew::Renderer::<StateBadge>::with_root_and_props(
            root.clone(),
            StateBadgeProps { state: WorkOrderState::EnRoute },
        )
        .render();
        tick().await;

        let span = root.query_selector("span").unwrap().expect("span mounted");
        let class = span.get_attribute("class").unwrap_or_default();
        assert!(class.contains("badge"),          "class={:?}", class);
        assert!(class.contains("state-EnRoute"),  "class={:?}", class);
        // Label is the human-readable form, not the enum variant.
        assert_eq!(span.text_content().unwrap_or_default(), "En Route");
    }

    #[wasm_bindgen_test]
    async fn priority_badge_renders_with_screaming_case_class() {
        let root = mount_root();
        yew::Renderer::<PriorityBadge>::with_root_and_props(
            root.clone(),
            PriorityBadgeProps { priority: Priority::Critical },
        )
        .render();
        tick().await;

        let span = root.query_selector("span").unwrap().expect("span mounted");
        let class = span.get_attribute("class").unwrap_or_default();
        assert!(class.contains("priority-CRITICAL"), "class={:?}", class);
        assert_eq!(span.text_content().unwrap_or_default(), "CRITICAL");
    }
}
