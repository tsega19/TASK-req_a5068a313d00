use std::cell::RefCell;
use std::rc::Rc;

use gloo_net::http::Method;
use uuid::Uuid;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use yew::prelude::*;

use crate::app::{toast_err, toast_ok, AuthCtx, ToastCtx};
use crate::components::loading_button::LoadingButton;
use crate::components::map_svg::MapSvg;
use crate::offline;
use crate::types::{Profile, TrailPoint, WorkOrder};

#[derive(Properties, PartialEq)]
pub struct MapProps {
    pub id: Uuid,
}

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct TrailResp {
    data: Vec<TrailPoint>,
    #[serde(default)]
    hidden: bool,
}

#[function_component(MapViewPage)]
pub fn map_view_page(props: &MapProps) -> Html {
    let auth = use_context::<AuthCtx>().expect("auth ctx");
    let toasts = use_context::<ToastCtx>().expect("toast ctx");

    let wo = use_state(|| None::<WorkOrder>);
    let trail = use_state(Vec::<TrailPoint>::new);
    let hidden = use_state(|| false);
    let profile = use_state(|| None::<Profile>);
    let toggling = use_state(|| false);
    let posting = use_state(|| false);
    let reload = use_state(|| 0u32);

    {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let wo = wo.clone();
        let trail = trail.clone();
        let hidden = hidden.clone();
        let profile = profile.clone();
        let id = props.id;
        let dep = *reload;
        use_effect_with(dep, move |_| {
            let state = auth.state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                // Offline-first reads so the map still shows last-known trail
                // points and pin location during a network drop.
                if let Ok(w) = offline::get_cached::<WorkOrder>(&format!("/api/work-orders/{}", id), &state).await {
                    wo.set(Some(w));
                }
                match offline::get_cached::<TrailResp>(
                    &format!("/api/work-orders/{}/location-trail", id),
                    &state,
                )
                .await
                {
                    Ok(r) => {
                        hidden.set(r.hidden);
                        trail.set(r.data);
                    }
                    Err(e) => toast_err(&toasts, format!("Trail load failed: {}", e.message)),
                }
                if let Ok(p) = offline::get_cached::<Profile>("/api/me", &state).await {
                    profile.set(Some(p));
                }
            });
            || ()
        });
    }

    let on_toggle_privacy = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let profile = profile.clone();
        let toggling = toggling.clone();
        let reload = reload.clone();
        Callback::from(move |_| {
            if *toggling {
                return;
            }
            toggling.set(true);
            let state = auth.state.clone();
            let toasts = toasts.clone();
            let current = profile.as_ref().map(|p| p.privacy_mode).unwrap_or(false);
            let new_val = !current;
            let profile = profile.clone();
            let toggling = toggling.clone();
            let reload = reload.clone();
            let reload_val = *reload;
            wasm_bindgen_futures::spawn_local(async move {
                let body = serde_json::json!({ "privacy_mode": new_val });
                match offline::mutate_with_queue(Method::PUT, "/api/me/privacy", &body, &state).await {
                    Ok(applied) => {
                        if let Some(mut p) = (*profile).clone() {
                            p.privacy_mode = new_val;
                            profile.set(Some(p));
                        }
                        let msg = if applied.is_none() {
                            if new_val {
                                "Privacy ON (queued — syncs on reconnect)"
                            } else {
                                "Privacy OFF (queued — syncs on reconnect)"
                            }
                        } else if new_val {
                            "Privacy mode ON — trail precision reduced."
                        } else {
                            "Privacy mode OFF — full trail precision."
                        };
                        toast_ok(&toasts, msg);
                        reload.set(reload_val + 1);
                    }
                    Err(e) => toast_err(&toasts, e.message),
                }
                toggling.set(false);
            });
        })
    };

    let on_capture = {
        let auth = auth.clone();
        let toasts = toasts.clone();
        let posting = posting.clone();
        let reload = reload.clone();
        let id = props.id;
        let wo_state = wo.clone();
        Callback::from(move |_| {
            if *posting {
                return;
            }
            posting.set(true);
            let auth = auth.clone();
            let toasts = toasts.clone();
            let posting = posting.clone();
            let reload = reload.clone();
            let wo_state = wo_state.clone();
            // Ask the browser for a real fix first; fall back to a point near the
            // job location if geolocation is unavailable (offline, denied, or
            // unsupported). The callback captures the handler state via Rc so a
            // second call after resolution can toast the correct outcome.
            request_location(move |result| {
                let state = auth.state.clone();
                let toasts = toasts.clone();
                let posting = posting.clone();
                let reload = reload.clone();
                let reload_val = *reload;
                let (lat, lng, source) = match result {
                    Ok((la, ln)) => (la, ln, "device"),
                    Err(msg) => {
                        // Offline / permission denied: fall back to the job
                        // location with small jitter so the trail still
                        // reflects something plausible. Warn the user so they
                        // know the captured point is synthetic.
                        toast_err(
                            &toasts,
                            format!("Geolocation unavailable ({}); using fallback", msg),
                        );
                        let (la, ln) = wo_state
                            .as_ref()
                            .and_then(|w| w.location_lat.zip(w.location_lng))
                            .map(|(la, ln)| {
                                (la + fastrand(-0.01, 0.01), ln + fastrand(-0.01, 0.01))
                            })
                            .unwrap_or((37.7749, -122.4194));
                        (la, ln, "fallback")
                    }
                };
                wasm_bindgen_futures::spawn_local(async move {
                    let body = serde_json::json!({ "lat": lat, "lng": lng });
                    let url = format!("/api/work-orders/{}/location-trail", id);
                    match offline::mutate_with_queue(Method::POST, &url, &body, &state).await {
                        Ok(Some(_)) => {
                            let msg = if source == "device" {
                                "Trail point recorded from device location"
                            } else {
                                "Trail point recorded (offline fallback)"
                            };
                            toast_ok(&toasts, msg);
                            reload.set(reload_val + 1);
                        }
                        Ok(None) => {
                            toast_ok(&toasts, "Trail point queued (offline)");
                            reload.set(reload_val + 1);
                        }
                        Err(e) => toast_err(&toasts, e.message),
                    }
                    posting.set(false);
                });
            });
        })
    };

    let privacy_on = profile.as_ref().map(|p| p.privacy_mode).unwrap_or(false);
    let reduced = privacy_on || trail.iter().any(|p| p.precision_reduced);

    html! {
        <div class="stack">
            <h1>{ "Map & trail" }</h1>
            <div class="row">
                <label class="switch">
                    <input type="checkbox" checked={privacy_on} onchange={on_toggle_privacy.clone()} disabled={*toggling} />
                    <span class="track"></span>
                    <span>{ if privacy_on { "Privacy mode ON" } else { "Privacy mode OFF" } }</span>
                </label>
                <LoadingButton
                    label="Capture point"
                    loading={*posting}
                    onclick={on_capture}
                    kind={Some("secondary".to_string())}
                />
                <span class="muted small right">
                    if *hidden { { "Trail hidden by owner privacy setting." } }
                </span>
            </div>

            <MapSvg
                trail={(*trail).clone()}
                pin_lat={wo.as_ref().and_then(|w| w.location_lat)}
                pin_lng={wo.as_ref().and_then(|w| w.location_lng)}
                reduced={reduced}
            />

            <div class="card">
                <h3>{ "Recent points" }</h3>
                if trail.is_empty() {
                    <p class="muted">{ "No points recorded yet." }</p>
                } else {
                    <table class="data">
                        <thead>
                            <tr><th>{"Time"}</th><th>{"Lat"}</th><th>{"Lng"}</th><th>{"Privacy"}</th></tr>
                        </thead>
                        <tbody>
                            { for trail.iter().rev().take(10).map(|p| html!{
                                <tr>
                                    <td>{ p.recorded_at.format("%H:%M:%S").to_string() }</td>
                                    <td>{ format!("{:.4}", p.lat) }</td>
                                    <td>{ format!("{:.4}", p.lng) }</td>
                                    <td>{ if p.precision_reduced { "Reduced" } else { "Full" } }</td>
                                </tr>
                            }) }
                        </tbody>
                    </table>
                }
            </div>
        </div>
    }
}

/// Tiny deterministic jitter — avoids pulling in the `rand` crate for wasm.
fn fastrand(min: f64, max: f64) -> f64 {
    let seed = js_sys::Date::now();
    let frac = (seed.fract() + seed.sin()).fract().abs();
    min + frac * (max - min)
}

/// Request a single high-accuracy fix from `navigator.geolocation`.
///
/// On success, `cb` is called with `Ok((lat, lng))`. On any failure
/// (unsupported, permission denied, offline, timeout) `cb` is called with
/// `Err(reason)` so the caller can fall back. The reason is user-facing.
///
/// The closures are held alive for the duration of the async browser call
/// via the `Rc<RefCell<Option<_>>>` dance — they drop themselves once either
/// callback fires.
fn request_location<F: 'static + FnOnce(Result<(f64, f64), String>)>(cb: F) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => {
            cb(Err("no window".into()));
            return;
        }
    };
    let geo = match window.navigator().geolocation() {
        Ok(g) => g,
        Err(_) => {
            cb(Err("geolocation unsupported".into()));
            return;
        }
    };

    let cb = Rc::new(RefCell::new(Some(cb)));

    let success_cb = cb.clone();
    let success = Closure::once(move |pos: wasm_bindgen::JsValue| {
        if let Some(cb) = success_cb.borrow_mut().take() {
            let pos: web_sys::Position = pos.unchecked_into();
            let coords = pos.coords();
            cb(Ok((coords.latitude(), coords.longitude())));
        }
    });

    let error_cb = cb.clone();
    let error = Closure::once(move |err: wasm_bindgen::JsValue| {
        if let Some(cb) = error_cb.borrow_mut().take() {
            let err: web_sys::PositionError = err.unchecked_into();
            cb(Err(err.message()));
        }
    });

    let mut opts = web_sys::PositionOptions::new();
    opts.enable_high_accuracy(true);
    opts.timeout(8_000);
    opts.maximum_age(30_000);

    let res = geo.get_current_position_with_error_callback_and_options(
        success.as_ref().unchecked_ref(),
        Some(error.as_ref().unchecked_ref()),
        &opts,
    );
    // Intentionally leak the closures — they must outlive this call but are
    // one-shots. `Closure::once` already arranges the single-call semantics;
    // forgetting keeps them alive until the browser invokes one.
    success.forget();
    error.forget();
    if res.is_err() {
        if let Some(cb) = cb.borrow_mut().take() {
            cb(Err("geolocation call failed".into()));
        }
    }
}
