//! Offline ZIP+4 geocoding coverage (PRD §6). Asserts that a free-form address
//! resolves to the bundled index on create, and that the dedicated
//! `/api/location/geocode` endpoint returns canonical + coordinates.

use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, make_service_with_cfg, raw_of, setup};

#[actix_web::test]
async fn create_work_order_normalizes_address_from_bundled_index() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "title": "Geocoded job",
            "location_address_norm": "123 Fake, SF, CA 94103-0001"
        }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    assert!(
        body["location_address_norm"]
            .as_str()
            .unwrap()
            .contains("SAN FRANCISCO"),
        "address must be normalized to canonical form, got {:?}",
        body["location_address_norm"]
    );
    let lat = body["location_lat"].as_f64().unwrap();
    assert!((lat - 37.7723).abs() < 1e-4, "lat should come from index, got {}", lat);
}

#[actix_web::test]
async fn geocode_endpoint_resolves_zip4() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/location/geocode")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "zip4": "94103-0001", "street": "Bryant St" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["from_index"], true);
    assert!((body["lat"].as_f64().unwrap() - 37.7723).abs() < 1e-4);
}

#[actix_web::test]
async fn geocode_endpoint_rejects_non_supervisor() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/location/geocode")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "query": "anything" }))
        .to_request();
    let (status, _) = raw_of(&app, req).await;
    assert_eq!(status, 403);
}

// -----------------------------------------------------------------------------
// Strict-mode fallback gate: when `allow_geocode_fallback = false`, unknown
// addresses MUST be rejected instead of synthesizing hash-derived coordinates.
// (Audit Medium #3 / config: ALLOW_GEOCODE_FALLBACK.)
// -----------------------------------------------------------------------------

#[actix_web::test]
async fn geocode_endpoint_rejects_unknown_address_in_strict_mode() {
    let ctx = setup().await;
    let mut cfg = ctx.cfg.clone();
    cfg.app.allow_geocode_fallback = false;
    let app = make_service_with_cfg(&ctx, cfg).await;
    let req = TestRequest::post()
        .uri("/api/location/geocode")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "query": "somewhere totally unknown" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 400);
    assert_eq!(body["code"], "bad_request");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not found in bundled ZIP+4 index"),
        "expected strict-mode rejection message, got {:?}",
        body["error"]
    );
}

#[actix_web::test]
async fn geocode_endpoint_still_allows_indexed_address_in_strict_mode() {
    // Strict mode must NOT break the happy path — indexed queries still
    // resolve deterministically, only the hash fallback is blocked.
    let ctx = setup().await;
    let mut cfg = ctx.cfg.clone();
    cfg.app.allow_geocode_fallback = false;
    let app = make_service_with_cfg(&ctx, cfg).await;
    let req = TestRequest::post()
        .uri("/api/location/geocode")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "zip4": "94103-0001", "street": "Bryant St" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["from_index"], true);
}

#[actix_web::test]
async fn create_work_order_rejects_unknown_address_in_strict_mode() {
    let ctx = setup().await;
    let mut cfg = ctx.cfg.clone();
    cfg.app.allow_geocode_fallback = false;
    let app = make_service_with_cfg(&ctx, cfg).await;
    let req = TestRequest::post()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "title": "Strict-mode WO",
            "location_address_norm": "somewhere totally unknown",
        }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 400);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not found in bundled ZIP+4 index"),
        "expected strict-mode rejection, got {:?}",
        body["error"]
    );
}
