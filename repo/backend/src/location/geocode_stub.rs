//! Offline geocoder backed by a bundled ZIP+4 + street index (PRD §6).
//!
//! The index is a small CSV under `backend/data/zip4_index.csv`. A real
//! deployment ships a larger USPS-derived dataset the same way; the loading
//! code does not care about size. Two lookup modes:
//!
//!   1. `normalize(zip4, street)` — authoritative, returns the canonical
//!      record from the bundled index (name + coordinates).
//!   2. `geocode(freeform_query)` — scans the query for a ZIP+4 token and
//!      a street name, matches the index row, and returns the row's
//!      coordinates. When no row matches, falls back to a deterministic
//!      hash-based point inside the continental-US bounding box so higher
//!      layers can still exercise the normalization path end-to-end.
//!
//! The fallback is reserved for the "unknown address" path — real addresses
//! that match the index resolve to the index's lat/lng and canonical name.

use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};

pub struct GeocodeResult {
    pub address_norm: String,
    pub lat: f64,
    pub lng: f64,
    /// True when the result came from the bundled index; false when it fell
    /// back to the deterministic hash placeholder for an unknown address.
    pub from_index: bool,
}

#[derive(Debug, Clone)]
struct IndexRow {
    zip4: String,
    street: String,
    city: String,
    state: String,
    lat: f64,
    lng: f64,
}

const ZIP4_CSV: &str = include_str!("../../data/zip4_index.csv");

static INDEX: Lazy<Vec<IndexRow>> = Lazy::new(load_index);
static ZIP4_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\d{5}-\d{4}\b").expect("zip4 regex"));

fn load_index() -> Vec<IndexRow> {
    let mut out = Vec::new();
    for (i, line) in ZIP4_CSV.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            // header or blank
            continue;
        }
        let mut cols = line.split(',');
        let zip4 = match cols.next() {
            Some(s) => s.trim().to_string(),
            None => continue,
        };
        let street = cols.next().unwrap_or("").trim().to_string();
        let city = cols.next().unwrap_or("").trim().to_string();
        let state = cols.next().unwrap_or("").trim().to_string();
        let lat: f64 = cols.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
        let lng: f64 = cols.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
        if zip4.is_empty() || street.is_empty() {
            continue;
        }
        out.push(IndexRow { zip4, street, city, state, lat, lng });
    }
    out
}

/// Authoritative lookup. Returns the bundled row when the ZIP+4 matches and
/// (optionally) the street name is consistent. Case-insensitive, trims both
/// inputs.
pub fn normalize(zip4: &str, street: Option<&str>) -> Option<GeocodeResult> {
    let zip4 = zip4.trim().to_ascii_uppercase();
    let row = INDEX.iter().find(|r| r.zip4.eq_ignore_ascii_case(&zip4))?;
    if let Some(s) = street {
        if !row
            .street
            .to_ascii_uppercase()
            .contains(&s.trim().to_ascii_uppercase())
            && !s
                .trim()
                .to_ascii_uppercase()
                .contains(&row.street.to_ascii_uppercase())
        {
            return None;
        }
    }
    Some(GeocodeResult {
        address_norm: format!("{}, {}, {} {}", row.street, row.city, row.state, row.zip4),
        lat: row.lat,
        lng: row.lng,
        from_index: true,
    })
}

pub fn geocode(input: &str) -> GeocodeResult {
    let trimmed = input.trim();

    // 1) Extract a ZIP+4 from the query, if present.
    if let Some(m) = ZIP4_RE.find(trimmed) {
        if let Some(hit) = normalize(m.as_str(), Some(trimmed)) {
            return hit;
        }
        if let Some(hit) = normalize(m.as_str(), None) {
            return hit;
        }
    }

    // 2) Street-name substring match against the index.
    let upper = trimmed.to_ascii_uppercase();
    for row in INDEX.iter() {
        if upper.contains(&row.street.to_ascii_uppercase()) {
            return GeocodeResult {
                address_norm: format!(
                    "{}, {}, {} {}",
                    row.street, row.city, row.state, row.zip4
                ),
                lat: row.lat,
                lng: row.lng,
                from_index: true,
            };
        }
    }

    // 3) Unknown address — deterministic hash fallback so callers get a
    //    stable point for previously-unseen inputs rather than a failure.
    let mut h = Sha256::new();
    h.update(trimmed.as_bytes());
    let digest = h.finalize();
    let a = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) as f64;
    let b = u32::from_be_bytes([digest[4], digest[5], digest[6], digest[7]]) as f64;
    let lat = 25.0 + (a / u32::MAX as f64) * (49.0 - 25.0);
    let lng = -125.0 + (b / u32::MAX as f64) * (-67.0 - -125.0);
    GeocodeResult {
        address_norm: trimmed.to_uppercase(),
        lat,
        lng,
        from_index: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_loaded_nonempty() {
        assert!(!INDEX.is_empty(), "bundled ZIP+4 index should load");
    }

    #[test]
    fn normalize_returns_known_row() {
        let r = normalize("94103-0001", Some("Bryant St")).unwrap();
        assert!(r.from_index);
        assert_eq!(r.lat, 37.7723);
        assert_eq!(r.lng, -122.4091);
        assert!(r.address_norm.contains("SAN FRANCISCO"));
    }

    #[test]
    fn normalize_unknown_zip_returns_none() {
        assert!(normalize("00000-0000", None).is_none());
    }

    #[test]
    fn geocode_recognizes_embedded_zip4() {
        let r = geocode("123 Fake, SF, CA 94103-0001");
        assert!(r.from_index);
        assert!((r.lat - 37.7723).abs() < 1e-6);
    }

    #[test]
    fn geocode_falls_back_for_unknown() {
        let r = geocode("somewhere totally unknown");
        assert!(!r.from_index);
        // Still inside the US bounding box — hash fallback.
        assert!(r.lat >= 25.0 && r.lat <= 49.0);
        assert!(r.lng >= -125.0 && r.lng <= -67.0);
    }

    #[test]
    fn deterministic_for_same_input() {
        let a = geocode("123 main st");
        let b = geocode("123 main st");
        assert_eq!(a.address_norm, b.address_norm);
        assert_eq!(a.lat, b.lat);
        assert_eq!(a.lng, b.lng);
    }
}
