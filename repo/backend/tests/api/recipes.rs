use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

#[actix_web::test]
async fn list_recipes_visible_to_any_authed_user() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/recipes")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["total"].as_i64().unwrap() >= 1);
}

#[actix_web::test]
async fn list_recipe_steps_returns_order() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/recipes/{}/steps", ctx.recipe_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let arr = body["data"].as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["step_order"], 1);
    assert_eq!(arr[2]["step_order"], 3);
}

#[actix_web::test]
async fn tech_cannot_create_tip_card() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/tip-cards")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "step_id": ctx.step_ids[0],
            "title": "Tip",
            "content": "Use torque wrench"
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn admin_creates_tip_card_and_tech_reads_it() {
    let ctx = setup().await;
    let step = ctx.step_ids[0];

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/tip-cards")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "step_id": step,
            "title": "Torque",
            "content": "Use torque wrench at 45 N·m"
        }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    let card_id = body["id"].as_str().unwrap().to_string();

    // Tech reads it.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri(&format!("/api/steps/{}/tip-cards", step))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status2, body2): (u16, serde_json::Value) = json_of(&app2, req2).await;
    assert_eq!(status2, 200);
    let arr = body2["data"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], card_id);

    // Admin updates it.
    let app3 = make_service(&ctx).await;
    let req3 = TestRequest::put()
        .uri(&format!("/api/tip-cards/{}", card_id))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "title": "Torque (v2)" }))
        .to_request();
    let (status3, body3) = raw_of(&app3, req3).await;
    assert_eq!(status3, 200);
    assert_eq!(body3["title"], "Torque (v2)");
}

#[actix_web::test]
async fn list_step_timers_returns_backend_defined_rows() {
    let ctx = setup().await;
    let step = ctx.step_ids[0];

    // Seed two timers for the step.
    sqlx::query(
        "INSERT INTO step_timers (step_id, label, duration_seconds, alert_type)
         VALUES ($1, 'Refrigerant stabilization', 300, 'BOTH'),
                ($1, 'Thermal cycle', 120, 'VISUAL')",
    )
    .bind(step)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/steps/{}/timers", step))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let arr = body["data"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // Rows are labelled (proves the label/duration/alert_type fields populate).
    let labels: Vec<&str> = arr.iter().map(|r| r["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"Refrigerant stabilization"));
    assert!(labels.contains(&"Thermal cycle"));
}

#[actix_web::test]
async fn timer_state_snapshot_round_trip() {
    let ctx = setup().await;
    let step = ctx.step_ids[0];
    let app = make_service(&ctx).await;

    // Persist a snapshot via PUT /progress.
    let snapshot = json!([
        { "timer_id": uuid::Uuid::new_v4(), "remaining_seconds": 42, "running": true }
    ]);
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/steps/{}/progress", ctx.wo_a_id, step))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "status": "InProgress",
            "notes": null,
            "timer_state": snapshot
        }))
        .to_request();
    let (status, _body) = raw_of(&app, req).await;
    assert_eq!(status, 200);

    // Read it back.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri(&format!("/api/work-orders/{}/progress", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status2, body2): (u16, serde_json::Value) = json_of(&app2, req2).await;
    assert_eq!(status2, 200);
    let row = body2["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["step_id"].as_str().unwrap() == step.to_string())
        .unwrap()
        .clone();
    assert_eq!(row["timer_state_snapshot"][0]["remaining_seconds"], 42);
    assert_eq!(row["timer_state_snapshot"][0]["running"], true);
}

#[actix_web::test]
async fn tip_card_update_404_when_missing() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/tip-cards/{}", uuid::Uuid::new_v4()))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "title": "x" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}
