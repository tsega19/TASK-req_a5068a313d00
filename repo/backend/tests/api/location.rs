use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

#[actix_web::test]
async fn post_trail_point_ok_for_owning_tech() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/location-trail", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "lat": 37.77, "lng": -122.42 }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    assert_eq!(body["precision_reduced"], false);
}

#[actix_web::test]
async fn post_trail_point_with_privacy_mode_reduces_precision() {
    let ctx = setup().await;
    sqlx::query("UPDATE users SET privacy_mode = TRUE WHERE id = $1")
        .bind(ctx.tech_a_id)
        .execute(&ctx.pool)
        .await
        .unwrap();
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/location-trail", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "lat": 37.7749, "lng": -122.4194 }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    assert_eq!(body["precision_reduced"], true);
    assert_eq!(body["lat"].as_f64().unwrap(), 37.77);
}

#[actix_web::test]
async fn non_owner_tech_cannot_post_trail_point() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/location-trail", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_b_token))
        .set_json(json!({ "lat": 1.0, "lng": 1.0 }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn trail_get_hidden_from_super_when_owner_privacy_on() {
    let ctx = setup().await;
    sqlx::query("UPDATE users SET privacy_mode = TRUE WHERE id = $1")
        .bind(ctx.tech_a_id)
        .execute(&ctx.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO location_trails (work_order_id, user_id, lat, lng) VALUES ($1, $2, $3, $4)",
    )
    .bind(ctx.wo_a_id)
    .bind(ctx.tech_a_id)
    .bind(37.77)
    .bind(-122.42)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/work-orders/{}/location-trail", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["hidden"], true);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

#[actix_web::test]
async fn trail_get_masks_for_super_when_privacy_off() {
    let ctx = setup().await;
    sqlx::query(
        "INSERT INTO location_trails (work_order_id, user_id, lat, lng) VALUES ($1, $2, $3, $4)",
    )
    .bind(ctx.wo_a_id)
    .bind(ctx.tech_a_id)
    .bind(37.7749)
    .bind(-122.4194)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/work-orders/{}/location-trail", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let pt = &body["data"][0];
    assert_eq!(pt["precision_reduced"], true);
    assert_eq!(pt["lat"].as_f64().unwrap(), 37.77);
}

#[actix_web::test]
async fn arrival_check_in_within_radius_ok() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/check-in", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "type": "ARRIVAL", "lat": 37.7750, "lng": -122.4190 }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 201);
}

#[actix_web::test]
async fn arrival_check_in_outside_radius_400() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/check-in", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "type": "ARRIVAL", "lat": 0.0, "lng": 0.0 }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn departure_check_in_skips_radius_validation() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/check-in", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "type": "DEPARTURE", "lat": 0.0, "lng": 0.0 }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 201);
}
