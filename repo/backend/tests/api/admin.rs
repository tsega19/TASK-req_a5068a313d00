use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

#[actix_web::test]
async fn list_users_requires_admin() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/admin/users")
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn admin_creates_user_with_valid_payload() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/users")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "username": "new_tech",
            "password": "correct-horse-battery-staple",
            "role": "TECH",
            "branch_id": ctx.branch_a_id
        }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    assert_eq!(body["username"], "new_tech");
    assert_eq!(body["role"], "TECH");
}

#[actix_web::test]
async fn admin_user_create_rejects_short_password() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/users")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "username": "x", "password": "ab", "role": "TECH" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn admin_user_create_rejects_duplicate_username() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/users")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "username": "admin", "password": "correct-horse-battery-staple", "role": "TECH" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 409);
}

#[actix_web::test]
async fn admin_cannot_delete_self() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/admin/users/{}", ctx.admin_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn admin_soft_deletes_other_user() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/admin/users/{}", ctx.tech_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 204);
    let dt: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM users WHERE id = $1")
            .bind(ctx.tech_a_id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(dt.is_some());
}

#[actix_web::test]
async fn admin_updates_user_role_and_privacy() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/admin/users/{}", ctx.tech_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "role": "SUPER", "privacy_mode": true }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["role"], "SUPER");
    assert_eq!(body["privacy_mode"], true);
}

#[actix_web::test]
async fn admin_branches_crud_lifecycle() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;

    let req = TestRequest::post()
        .uri("/api/admin/branches")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "name": "Branch C", "service_radius_miles": 20 }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 201);
    let id = body["id"].as_str().unwrap().to_string();

    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::put()
        .uri(&format!("/api/admin/branches/{}", id))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "service_radius_miles": 50 }))
        .to_request();
    let (status2, body2) = raw_of(&app2, req2).await;
    assert_eq!(status2, 200);
    assert_eq!(body2["service_radius_miles"], 50);

    let app3 = make_service(&ctx).await;
    let req3 = TestRequest::get()
        .uri("/api/admin/branches")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status3, body3): (u16, serde_json::Value) = json_of(&app3, req3).await;
    assert_eq!(status3, 200);
    assert!(body3["total"].as_i64().unwrap() >= 3);
}

#[actix_web::test]
async fn sync_trigger_returns_report() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/sync/trigger")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body.get("work_orders_scanned").is_some());
    assert!(body.get("conflicts_flagged").is_some());
}

#[actix_web::test]
async fn sync_trigger_rejects_non_admin() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/sync/trigger")
        .insert_header(auth_header(&ctx.super_token))
        .set_json(json!({}))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}
