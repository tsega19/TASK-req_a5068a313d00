//! Multi-concurrent step timer.
//!
//! Visual: SVG ring, stroke-dasharray countdown.
//! Audible: stub via Web Audio API OscillatorNode (≈700ms 880 Hz beep at 0s).
//!
//! Each timer is a self-contained component — mount many in parallel and
//! they each tick independently. Parents can persist the tick state by
//! subscribing to `on_tick` and writing the resulting snapshots back to the
//! backend (see `recipe_step.rs`).

use gloo_timers::callback::Interval;
use std::cell::RefCell;
use std::rc::Rc;
use uuid::Uuid;
use wasm_bindgen::JsCast;
use web_sys::{AudioContext, OscillatorType};
use yew::prelude::*;

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TimerSnapshot {
    pub timer_id: Uuid,
    pub remaining_seconds: i32,
    pub running: bool,
}

#[derive(Properties, PartialEq, Clone)]
pub struct TimerRingProps {
    #[prop_or_default]
    pub timer_id: Option<Uuid>,
    pub label: String,
    pub duration_seconds: u32,
    #[prop_or("BOTH".into())]
    pub alert_type: String, // AUDIBLE | VISUAL | BOTH
    #[prop_or_default]
    pub autostart: bool,
    /// Seconds remaining when this ring mounts. When `None`, the ring starts
    /// from `duration_seconds`.
    #[prop_or_default]
    pub initial_remaining: Option<u32>,
    /// Whether this ring is running (counting down) when it mounts.
    #[prop_or_default]
    pub initial_running: bool,
    /// Fires every time the internal state changes (tick, start/pause, reset)
    /// so the parent can persist the snapshot. Only emitted when `timer_id`
    /// is set.
    #[prop_or_default]
    pub on_tick: Callback<TimerSnapshot>,
}

#[function_component(TimerRing)]
pub fn timer_ring(props: &TimerRingProps) -> Html {
    let remaining = use_state(|| props.initial_remaining.unwrap_or(props.duration_seconds));
    let running = use_state(|| props.initial_running || props.autostart);
    let fired = use_state(|| false);

    // Reset when duration prop changes.
    {
        let remaining = remaining.clone();
        let duration = props.duration_seconds;
        let initial_remaining = props.initial_remaining;
        let fired = fired.clone();
        use_effect_with(duration, move |d| {
            remaining.set(initial_remaining.unwrap_or(*d));
            fired.set(false);
            || ()
        });
    }

    let emit = {
        let on_tick = props.on_tick.clone();
        let timer_id = props.timer_id;
        move |remaining: u32, running: bool| {
            if let Some(id) = timer_id {
                on_tick.emit(TimerSnapshot {
                    timer_id: id,
                    remaining_seconds: remaining as i32,
                    running,
                });
            }
        }
    };

    {
        let remaining = remaining.clone();
        let running = running.clone();
        let fired = fired.clone();
        let alert_type = props.alert_type.clone();
        let emit_tick = emit.clone();
        use_effect_with((), move |_| {
            let r = remaining.clone();
            let run = running.clone();
            let done = fired.clone();
            let at = alert_type.clone();
            let emit_tick = emit_tick.clone();
            let interval = Interval::new(1000, move || {
                if !*run {
                    return;
                }
                let cur = *r;
                if cur == 0 {
                    if !*done {
                        done.set(true);
                        if at != "VISUAL" {
                            play_beep();
                        }
                    }
                    return;
                }
                let next = cur - 1;
                r.set(next);
                emit_tick(next, true);
            });
            move || drop(interval)
        });
    }

    let total = props.duration_seconds.max(1) as f64;
    let remaining_f = *remaining as f64;
    let pct_remaining = (remaining_f / total).clamp(0.0, 1.0);
    const CIRCUM: f64 = 2.0 * std::f64::consts::PI * 50.0; // r=50
    let offset = CIRCUM * (1.0 - pct_remaining);

    let alerting = *remaining == 0 && props.alert_type != "AUDIBLE";
    let done = *remaining == 0;
    let ring_class = if alerting {
        "timer-ring alerting"
    } else if done {
        "timer-ring done"
    } else {
        "timer-ring"
    };

    let toggle = {
        let running = running.clone();
        let remaining = remaining.clone();
        let emit_tick = emit.clone();
        Callback::from(move |_| {
            let next = !*running;
            running.set(next);
            emit_tick(*remaining, next);
        })
    };
    let reset = {
        let remaining = remaining.clone();
        let running = running.clone();
        let fired = fired.clone();
        let duration = props.duration_seconds;
        let emit_tick = emit.clone();
        Callback::from(move |_| {
            remaining.set(duration);
            running.set(false);
            fired.set(false);
            emit_tick(duration, false);
        })
    };

    html! {
        <div class="timer-card">
            <div class="small muted">{ &props.label }</div>
            <div class={ring_class}>
                <svg viewBox="0 0 120 120">
                    <circle class="bg-arc" cx="60" cy="60" r="50" />
                    <circle
                        class="fg-arc"
                        cx="60" cy="60" r="50"
                        stroke-dasharray={format!("{:.2}", CIRCUM)}
                        stroke-dashoffset={format!("{:.2}", offset)}
                    />
                </svg>
                <div class="readout">{ format_mmss(*remaining) }</div>
            </div>
            <div class="controls">
                <button class="secondary" onclick={toggle}>
                    { if *running { "Pause" } else { "Start" } }
                </button>
                <button class="ghost" onclick={reset}>{ "Reset" }</button>
            </div>
        </div>
    }
}

fn format_mmss(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    #[wasm_bindgen_test]
    fn format_mmss_pads_minutes_and_seconds() {
        assert_eq!(format_mmss(0), "00:00");
        assert_eq!(format_mmss(9), "00:09");
        assert_eq!(format_mmss(59), "00:59");
        assert_eq!(format_mmss(60), "01:00");
        assert_eq!(format_mmss(3_599), "59:59");
        // Long bakes that exceed an hour still display with two-digit minutes
        // — the UI renders them as "60:00", not "1:00:00". That's deliberate;
        // technicians compare ring-to-ring, and re-padding to H:MM:SS would
        // change the visual width mid-bake.
        assert_eq!(format_mmss(3_600), "60:00");
    }

    #[wasm_bindgen_test]
    fn timer_snapshot_roundtrips_through_serde() {
        let id = uuid::Uuid::new_v4();
        let s = TimerSnapshot { timer_id: id, remaining_seconds: 45, running: true };
        let json = serde_json::to_string(&s).unwrap();
        let back: TimerSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timer_id, id);
        assert_eq!(back.remaining_seconds, 45);
        assert!(back.running);
    }
}

// -----------------------------------------------------------------------------
// Audible stub: a single 880Hz beep via Web Audio — no external audio service.
// Wrapped in a thread-local so we reuse one AudioContext.
// -----------------------------------------------------------------------------
thread_local! {
    static CTX: RefCell<Option<Rc<AudioContext>>> = RefCell::new(None);
}

fn play_beep() {
    let ctx = CTX.with(|slot| {
        let mut s = slot.borrow_mut();
        if s.is_none() {
            if let Ok(c) = AudioContext::new() {
                *s = Some(Rc::new(c));
            }
        }
        s.clone()
    });
    let Some(ctx) = ctx else { return };
    let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else {
        return;
    };
    osc.set_type(OscillatorType::Sine);
    if let Some(freq) = osc.frequency().value().is_finite().then(|| osc.frequency()) {
        freq.set_value(880.0);
    }
    gain.gain().set_value(0.15);
    // Bind the destination node to a local so the `&AudioNode` borrow lives
    // long enough to be used by `connect_with_audio_node` below.
    let dest_node = ctx.destination();
    let dest: &web_sys::AudioNode = dest_node.unchecked_ref();
    let _ = osc.connect_with_audio_node(gain.unchecked_ref());
    let _ = gain.connect_with_audio_node(dest);
    let start_at = ctx.current_time();
    let _ = osc.start_with_when(start_at);
    let _ = osc.stop_with_when(start_at + 0.7);
}
