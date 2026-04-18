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
