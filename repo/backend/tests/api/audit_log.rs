//! Coverage for the immutable processing_log (PRD §7). Asserts that state-
//! changing user actions leave an audit row behind and that the DB trigger
//! rejects updates/deletes outside the retention-prune path.

use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, raw_of, setup, status_of};

#[actix_web::test]
async fn state_transition_writes_processing_log_row() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;

    // Tech A transitions WO-A Scheduled -> EnRoute.
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "to_state": "EnRoute",
            "notes": "heading out",
            "lat": 37.7749,
            "lng": -122.4194
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 200);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM processing_log
         WHERE action = 'work_order.transition'
           AND entity_id = $1
           AND user_id = $2",
    )
    .bind(ctx.wo_a_id)
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "transition must leave one processing_log row");
}

#[actix_web::test]
async fn check_in_writes_processing_log_row() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/check-in", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "type": "ARRIVAL", "lat": 37.7750, "lng": -122.4190 }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 201);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM processing_log
         WHERE action = 'check_in.create' AND user_id = $1",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(count >= 1);
}

#[actix_web::test]
async fn processing_log_is_immutable_at_db_level() {
    let ctx = setup().await;
    sqlx::query(
        "INSERT INTO processing_log (user_id, action, entity_table, entity_id, payload)
         VALUES ($1, 'test.seed', 'users', $1, '{}'::jsonb)",
    )
    .bind(ctx.admin_id)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let update_err = sqlx::query(
        "UPDATE processing_log SET action = 'tamper' WHERE user_id = $1",
    )
    .bind(ctx.admin_id)
    .execute(&ctx.pool)
    .await
    .err()
    .expect("update must be rejected by the immutability trigger");
    assert!(
        update_err.to_string().contains("immutable"),
        "expected immutability trigger, got: {}",
        update_err
    );

    let delete_err = sqlx::query(
        "DELETE FROM processing_log WHERE user_id = $1",
    )
    .bind(ctx.admin_id)
    .execute(&ctx.pool)
    .await
    .err()
    .expect("delete must be rejected by the immutability trigger");
    assert!(
        delete_err.to_string().contains("immutable"),
        "expected immutability trigger, got: {}",
        delete_err
    );
}

#[actix_web::test]
async fn login_writes_processing_log_row() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    // login request with valid credentials.
    let req = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "tech_a", "password": "pw" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 200);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM processing_log
         WHERE action = 'auth.login' AND user_id = $1",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(count >= 1, "login must leave a processing_log row");
}

#[actix_web::test]
async fn trail_point_writes_processing_log_row() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/work-orders/{}/location-trail", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "lat": 37.7749, "lng": -122.4194 }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 201);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM processing_log
         WHERE action = 'location_trail.append' AND user_id = $1",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(count >= 1, "trail append must leave a processing_log row");
}

#[actix_web::test]
async fn work_order_delete_writes_audit_atomically() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/work-orders/{}", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 204);

    let audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM processing_log
         WHERE action = 'work_order.delete' AND entity_id = $1",
    )
    .bind(ctx.wo_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1, "soft-delete must leave exactly one audit row");

    let tombstone: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log
         WHERE entity_table = 'work_orders'
           AND entity_id = $1
           AND operation = 'DELETE'",
    )
    .bind(ctx.wo_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(tombstone >= 1, "sync_log DELETE must land with audit row");
}

#[actix_web::test]
async fn admin_can_read_processing_log_non_admin_cannot() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;

    // Seed a row so the listing is nonempty.
    sqlx::query(
        "INSERT INTO processing_log (user_id, action, entity_table, entity_id, payload)
         VALUES ($1, 'test.seed', 'users', $1, '{}'::jsonb)",
    )
    .bind(ctx.admin_id)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let req = TestRequest::get()
        .uri("/api/admin/processing-log")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["total"].as_i64().unwrap() >= 1);

    let app2 = super::common::make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri("/api/admin/processing-log")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    assert_eq!(status_of(&app2, req2).await, 403);
}
