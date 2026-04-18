use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

#[actix_web::test]
async fn admin_lists_all_work_orders() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], 2);
}

#[actix_web::test]
async fn tech_sees_only_own_jobs() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["title"], "WO-A");
}

#[actix_web::test]
async fn super_sees_branch_jobs() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["branch_id"], ctx.branch_a_id.to_string());
}

#[actix_web::test]
async fn get_work_order_returns_404_for_non_owner_tech() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/work-orders/{}", ctx.wo_b_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn tech_cannot_create_work_order() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "title": "X", "branch_id": ctx.branch_a_id }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn super_creates_work_order() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.super_token))
        .set_json(json!({
            "title": "Fresh WO",
            "branch_id": ctx.branch_a_id,
            "priority": "NORMAL"
        }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    assert_eq!(body["title"], "Fresh WO");
    assert_eq!(body["state"], "Scheduled");
}

#[actix_web::test]
async fn create_wo_rejects_location_outside_branch_radius() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/work-orders")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "title": "Too Far",
            "branch_id": ctx.branch_a_id,
            "location_lat": 40.0,   // far from Branch A (SF)
            "location_lng": -100.0
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn transition_scheduled_to_enroute_requires_gps() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "to_state": "EnRoute" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn transition_happy_path_scheduled_to_enroute() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "to_state": "EnRoute", "lat": 37.77, "lng": -122.42 }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["state"], "EnRoute");
    assert_eq!(body["version_count"], 2);
}

#[actix_web::test]
async fn tech_cannot_cancel_work_order() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "to_state": "Canceled", "notes": "x" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn super_cancels_with_notes() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.super_token))
        .set_json(json!({ "to_state": "Canceled", "notes": "unreachable customer" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["state"], "Canceled");
}

#[actix_web::test]
async fn super_cancel_without_notes_is_400() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.super_token))
        .set_json(json!({ "to_state": "Canceled" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn timeline_reflects_transitions_with_body_content() {
    // Rule 3: the original timeline_includes_initial_transition test only
    // asserted "array exists". Strengthen by driving a real transition through
    // the API and asserting the recorded entry's from_state, to_state, and
    // triggering user round-trip verbatim.
    let ctx = setup().await;
    // Drive Scheduled -> EnRoute as tech_a via the state endpoint.
    let app0 = make_service(&ctx).await;
    let trans = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "to_state": "EnRoute",
            "lat": 37.7749,
            "lng": -122.4194
        }))
        .to_request();
    assert_eq!(status_of(&app0, trans).await, 200);

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/work-orders/{}/timeline", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    // Exactly one transition recorded; assert its shape + content.
    assert_eq!(rows.len(), 1, "timeline must contain the EnRoute transition");
    let entry = &rows[0];
    assert_eq!(entry["from_state"], "Scheduled");
    assert_eq!(entry["to_state"], "EnRoute");
    assert_eq!(entry["triggered_by"], ctx.tech_a_id.to_string());
    assert_eq!(entry["work_order_id"], ctx.wo_a_id.to_string());
    assert!(entry["created_at"].as_str().is_some());
    // required_fields echo the lat/lng that were supplied.
    let rf = &entry["required_fields"];
    assert!(rf["lat"].as_f64().is_some() || rf["lng"].as_f64().is_some());
}

#[actix_web::test]
async fn on_call_queue_requires_super_or_admin() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/work-orders/on-call-queue")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn on_call_queue_returns_high_priority_near_deadline() {
    let ctx = setup().await;
    // Re-tune WO-A's SLA to be close to breach.
    sqlx::query("UPDATE work_orders SET sla_deadline = NOW() + INTERVAL '1 hour' WHERE id = $1")
        .bind(ctx.wo_a_id)
        .execute(&ctx.pool)
        .await
        .unwrap();
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/work-orders/on-call-queue")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["total"].as_i64().unwrap_or(0) >= 1);
}

#[actix_web::test]
async fn admin_can_soft_delete_work_order() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/work-orders/{}", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 204);
    // Follow-up get returns 404 because deleted_at is now set.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri(&format!("/api/work-orders/{}", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app2, req2).await, 404);
}

#[actix_web::test]
async fn super_cannot_soft_delete_work_order() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/work-orders/{}", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn step_progress_upsert_creates_then_updates() {
    let ctx = setup().await;
    let step_id = ctx.step_ids[0];
    let app = make_service(&ctx).await;

    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/steps/{}/progress", ctx.wo_a_id, step_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "status": "InProgress" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "InProgress");
    assert_eq!(body["version"], 1);

    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/steps/{}/progress", ctx.wo_a_id, step_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "status": "Completed", "notes": "done" }))
        .to_request();
    let (status2, body2) = raw_of(&app2, req2).await;
    assert_eq!(status2, 200);
    assert_eq!(body2["status"], "Completed");
    assert_eq!(body2["version"], 2);
}
