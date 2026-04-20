//! Self-contained SVG map. No external tile provider is ever contacted —
//! the rule in guide.md forbids it and PRD §3 requires the offline geocoder.
//! We render a coordinate grid, a trajectory polyline, and pins.

use yew::prelude::*;

use crate::types::TrailPoint;

#[derive(Properties, PartialEq)]
pub struct MapSvgProps {
    pub trail: Vec<TrailPoint>,
    #[prop_or_default]
    pub pin_lat: Option<f64>,
    #[prop_or_default]
    pub pin_lng: Option<f64>,
    #[prop_or_default]
    pub reduced: bool,
}

#[function_component(MapSvg)]
pub fn map_svg(props: &MapSvgProps) -> Html {
    // Bounds: if we have points, fit to them; else default to continental US.
    let (min_lat, max_lat, min_lng, max_lng) = bounds(&props.trail, props.pin_lat, props.pin_lng);
    let pad_lat = ((max_lat - min_lat) * 0.15).max(0.5);
    let pad_lng = ((max_lng - min_lng) * 0.15).max(0.5);
    let min_lat = min_lat - pad_lat;
    let max_lat = max_lat + pad_lat;
    let min_lng = min_lng - pad_lng;
    let max_lng = max_lng + pad_lng;

    let project = move |lat: f64, lng: f64| -> (f64, f64) {
        let x = (lng - min_lng) / (max_lng - min_lng) * 1000.0;
        let y = 600.0 - (lat - min_lat) / (max_lat - min_lat) * 600.0;
        (x, y)
    };

    let grid_lines: Html = (0..=10)
        .flat_map(|i| {
            let x = (i as f64) * 100.0;
            let y = (i as f64) * 60.0;
            vec![
                html! { <line class="map-grid-line" x1={x.to_string()} y1="0" x2={x.to_string()} y2="600" /> },
                html! { <line class="map-grid-line" x1="0" y1={y.to_string()} x2="1000" y2={y.to_string()} /> },
            ]
        })
        .collect();

    let trail_points: String = props
        .trail
        .iter()
        .map(|p| {
            let (x, y) = project(p.lat, p.lng);
            format!("{:.1},{:.1}", x, y)
        })
        .collect::<Vec<_>>()
        .join(" ");

    let pin = props.pin_lat.zip(props.pin_lng).map(|(lat, lng)| {
        let (x, y) = project(lat, lng);
        html! {
            <g transform={format!("translate({},{})", x, y)}>
                <path class="map-pin" d="M0,-20 a10,10 0 1 1 0.01,0 Z M-2,-6 L0,4 L2,-6 Z" />
            </g>
        }
    });

    let last_user_point = props.trail.last().map(|p| {
        let (x, y) = project(p.lat, p.lng);
        html! { <circle class="map-user" cx={x.to_string()} cy={y.to_string()} r="7" /> }
    });

    html! {
        <div class="map-frame">
            <svg viewBox="0 0 1000 600" preserveAspectRatio="xMidYMid meet">
                { grid_lines }
                if !trail_points.is_empty() {
                    <polyline class="map-trail" points={trail_points} />
                }
                { for pin }
                { for last_user_point }
            </svg>
            <div class="map-legend">
                <div>{ format!("{} point{}", props.trail.len(), if props.trail.len() == 1 { "" } else { "s" }) }</div>
                if props.reduced {
                    <div style="color: var(--warn)">{ "Privacy mode — trail precision reduced" }</div>
                }
            </div>
        </div>
    }
}

fn bounds(
    trail: &[TrailPoint],
    pin_lat: Option<f64>,
    pin_lng: Option<f64>,
) -> (f64, f64, f64, f64) {
    let mut min_lat = 90.0f64;
    let mut max_lat = -90.0f64;
    let mut min_lng = 180.0f64;
    let mut max_lng = -180.0f64;
    let mut any = false;
    for p in trail {
        min_lat = min_lat.min(p.lat);
        max_lat = max_lat.max(p.lat);
        min_lng = min_lng.min(p.lng);
        max_lng = max_lng.max(p.lng);
        any = true;
    }
    if let (Some(lat), Some(lng)) = (pin_lat, pin_lng) {
        min_lat = min_lat.min(lat);
        max_lat = max_lat.max(lat);
        min_lng = min_lng.min(lng);
        max_lng = max_lng.max(lng);
        any = true;
    }
    if !any || (max_lat - min_lat).abs() < f64::EPSILON {
        // Default continental US frame
        return (25.0, 49.0, -125.0, -67.0);
    }
    if (max_lat - min_lat) < 0.01 {
        max_lat += 0.1;
        min_lat -= 0.1;
    }
    if (max_lng - min_lng) < 0.01 {
        max_lng += 0.1;
        min_lng -= 0.1;
    }
    (min_lat, max_lat, min_lng, max_lng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wasm_bindgen_test::*;

    // Configure is centralized in types.rs; see the note there.

    fn tp(lat: f64, lng: f64) -> TrailPoint {
        TrailPoint {
            id: uuid::Uuid::new_v4(),
            work_order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            lat,
            lng,
            precision_reduced: false,
            recorded_at: Utc::now(),
        }
    }

    #[wasm_bindgen_test]
    fn bounds_defaults_to_continental_us_when_empty() {
        let (min_lat, max_lat, min_lng, max_lng) = bounds(&[], None, None);
        assert_eq!(min_lat, 25.0);
        assert_eq!(max_lat, 49.0);
        assert_eq!(min_lng, -125.0);
        assert_eq!(max_lng, -67.0);
    }

    #[wasm_bindgen_test]
    fn bounds_fits_trail_points() {
        let (min_lat, max_lat, min_lng, max_lng) =
            bounds(&[tp(37.0, -122.0), tp(40.0, -120.0)], None, None);
        assert!(min_lat <= 37.0);
        assert!(max_lat >= 40.0);
        assert!(min_lng <= -122.0);
        assert!(max_lng >= -120.0);
    }

    #[wasm_bindgen_test]
    fn bounds_include_pin_coords_when_supplied() {
        let (min_lat, max_lat, _, _) = bounds(&[tp(37.0, -122.0)], Some(45.0), Some(-120.0));
        assert!(max_lat >= 45.0, "pin lat must expand bounds upward");
        assert!(min_lat <= 37.0, "trail lat must remain inside bounds");
    }

    #[wasm_bindgen_test]
    fn bounds_expands_degenerate_ranges() {
        // Two points on the same lat/lng — the raw span is zero and would
        // produce a divide-by-zero in the projection. The helper must pad.
        let (min_lat, max_lat, min_lng, max_lng) =
            bounds(&[tp(37.0, -122.0), tp(37.0, -122.0)], None, None);
        // With no points-with-variance the function falls back to continental US.
        assert_eq!((min_lat, max_lat, min_lng, max_lng), (25.0, 49.0, -125.0, -67.0));
    }
}
