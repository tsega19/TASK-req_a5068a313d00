//! SLA countdown badge — updates every 30 seconds. Turns orange at ≥75%
//! elapsed and red once the deadline is breached.

use chrono::{DateTime, Utc};
use gloo_timers::callback::Interval;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct SlaCountdownProps {
    pub deadline: Option<DateTime<Utc>>,
    #[prop_or_default]
    pub started: Option<DateTime<Utc>>,
}

#[function_component(SlaCountdown)]
pub fn sla_countdown(props: &SlaCountdownProps) -> Html {
    let tick = use_state(|| 0u32);

    {
        let tick = tick.clone();
        use_effect_with((), move |_| {
            let interval = Interval::new(30_000, move || tick.set(*tick + 1));
            move || drop(interval)
        });
    }

    let _ = *tick;
    let Some(deadline) = props.deadline else {
        return html! { <span class="sla-countdown muted">{"no SLA"}</span> };
    };

    let now = Utc::now();
    let delta = deadline - now;
    let total_ms = delta.num_milliseconds();
    let breached = total_ms < 0;

    let started = props.started.unwrap_or(now - chrono::Duration::hours(24));
    let window_ms = (deadline - started).num_milliseconds().max(1);
    let elapsed_ms = (now - started).num_milliseconds();
    let pct = (elapsed_ms as f64 / window_ms as f64).clamp(0.0, 2.0);

    let class = if breached {
        "sla-countdown breach"
    } else if pct >= 0.75 {
        "sla-countdown warn"
    } else {
        "sla-countdown"
    };

    let label = if breached {
        format!("SLA breach +{}", fmt_delta(-total_ms))
    } else {
        format!("SLA {}", fmt_delta(total_ms))
    };

    html! { <span class={class}>{ label }</span> }
}

fn fmt_delta(ms: i64) -> String {
    let secs = ms / 1000;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{}h {:02}m", h, m)
    } else {
        format!("{}m", m.max(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn fmt_delta_under_an_hour_shows_minutes_only() {
        assert_eq!(fmt_delta(0), "0m");
        assert_eq!(fmt_delta(59_999), "0m");
        assert_eq!(fmt_delta(60_000), "1m");
        assert_eq!(fmt_delta(59 * 60_000), "59m");
    }

    #[wasm_bindgen_test]
    fn fmt_delta_one_hour_and_up_shows_h_mm() {
        assert_eq!(fmt_delta(3_600_000), "1h 00m");
        assert_eq!(fmt_delta(3_600_000 + 5 * 60_000), "1h 05m");
        assert_eq!(fmt_delta(25 * 3_600_000), "25h 00m");
    }

    #[wasm_bindgen_test]
    fn fmt_delta_clamps_negative_minutes_to_zero() {
        // The SLA badge formatter is called with both positive remaining and
        // negative-sign-flipped-breach magnitudes; the sub-hour branch must
        // never render "-0m" or similar noise.
        assert_eq!(fmt_delta(-100), "0m");
    }

    // ---------------------------------------------------------------
    // Real render tests — the countdown component's *class* and
    // *text* are behavioral outputs, not just helper returns. Pin
    // them by mounting the component and querying the DOM.
    // ---------------------------------------------------------------

    async fn tick() {
        gloo_timers::future::TimeoutFuture::new(30).await;
    }

    fn mount_root() -> web_sys::Element {
        let doc = web_sys::window().unwrap().document().unwrap();
        let root = doc.create_element("div").unwrap();
        doc.body().unwrap().append_child(&root).unwrap();
        root
    }

    #[wasm_bindgen_test]
    async fn sla_countdown_renders_no_sla_when_deadline_missing() {
        let root = mount_root();
        yew::Renderer::<SlaCountdown>::with_root_and_props(
            root.clone(),
            SlaCountdownProps { deadline: None, started: None },
        )
        .render();
        tick().await;

        let span = root.query_selector("span").unwrap().expect("span mounted");
        let class = span.get_attribute("class").unwrap_or_default();
        assert!(class.contains("muted"), "class={:?}", class);
        assert_eq!(span.text_content().unwrap_or_default(), "no SLA");
    }

    #[wasm_bindgen_test]
    async fn sla_countdown_renders_breach_class_for_past_deadline() {
        let root = mount_root();
        // Deadline one hour in the past, started two hours ago — `elapsed > window`
        // so the component must pick the `breach` class and the text starts with
        // "SLA breach".
        let deadline = Utc::now() - chrono::Duration::hours(1);
        let started = Utc::now() - chrono::Duration::hours(2);
        yew::Renderer::<SlaCountdown>::with_root_and_props(
            root.clone(),
            SlaCountdownProps { deadline: Some(deadline), started: Some(started) },
        )
        .render();
        tick().await;

        let span = root.query_selector("span").unwrap().expect("span mounted");
        let class = span.get_attribute("class").unwrap_or_default();
        assert!(class.contains("breach"), "class={:?}", class);
        let text = span.text_content().unwrap_or_default();
        assert!(text.starts_with("SLA breach"), "text={:?}", text);
    }
}
