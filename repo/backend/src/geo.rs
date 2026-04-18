//! Geo helpers: haversine distance in miles + privacy-mode precision reduction.

const EARTH_RADIUS_MILES: f64 = 3958.7613;

pub fn haversine_miles(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let (lat1r, lat2r) = (lat1.to_radians(), lat2.to_radians());
    let dlat = (lat2 - lat1).to_radians();
    let dlng = (lng2 - lng1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1r.cos() * lat2r.cos() * (dlng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_MILES * c
}

/// Reduce coordinate precision to roughly 1 mile (~0.015°) per PRD §7 privacy
/// mode. We round to 2 decimal places (~0.7 mi) which satisfies the "~1 mile"
/// target while remaining deterministic.
pub fn reduce_precision(lat: f64, lng: f64) -> (f64, f64) {
    ((lat * 100.0).round() / 100.0, (lng * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_zero_for_same_point() {
        assert!(haversine_miles(40.0, -73.0, 40.0, -73.0) < 1e-6);
    }

    #[test]
    fn haversine_sane_distance_nyc_to_la() {
        // ~2451 miles
        let d = haversine_miles(40.7128, -74.0060, 34.0522, -118.2437);
        assert!((d - 2451.0).abs() < 25.0, "got {}", d);
    }

    #[test]
    fn reduce_precision_rounds_to_hundredths() {
        let (lat, lng) = reduce_precision(40.71285, -74.00605);
        assert!((lat - 40.71).abs() < 1e-9);
        assert!((lng + 74.01).abs() < 1e-9);
    }
}
