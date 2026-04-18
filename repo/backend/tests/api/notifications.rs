use actix_web::test::TestRequest;
use serde_json::json;
use uuid::Uuid;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

async fn seed_notification(pool: &sqlx::PgPool, user_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO notifications (user_id, template_type, payload)
         VALUES ($1, 'SCHEDULE_CHANGE', '{\"wo\":\"x\"}'::jsonb)
         RETURNING id",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[actix_web::test]
async fn list_notifications_returns_own_only() {
    let ctx = setup().await;
    let _mine = seed_notification(&ctx.pool, ctx.tech_a_id).await;
    let _theirs = seed_notification(&ctx.pool, ctx.tech_b_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/notifications")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["user_id"], ctx.tech_a_id.to_string());
}

#[actix_web::test]
async fn mark_read_updates_row() {
    let ctx = setup().await;
    let id = seed_notification(&ctx.pool, ctx.tech_a_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/notifications/{}/read", id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({}))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["read_at"].is_string());
}

#[actix_web::test]
async fn mark_read_rejects_other_users_notifications() {
    let ctx = setup().await;
    let id = seed_notification(&ctx.pool, ctx.tech_b_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/notifications/{}/read", id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({}))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn unsubscribe_records_preference() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri("/api/notifications/unsubscribe")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "template_type": "SCHEDULE_CHANGE" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_unsubscribes
         WHERE user_id = $1 AND template_type = 'SCHEDULE_CHANGE'",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[actix_web::test]
async fn retry_delivers_pending_row_past_backoff() {
    let ctx = setup().await;
    // Seed a pending row (no delivered_at, retry_count=0, created a while ago).
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications
            (user_id, template_type, payload, retry_count, created_at)
         VALUES ($1, 'SCHEDULE_CHANGE', '{}'::jsonb, 0, NOW() - INTERVAL '10 seconds')
         RETURNING id",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/notifications/retry")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["delivered"].as_i64().unwrap() >= 1);

    let (delivered_at, retry_count): (Option<chrono::DateTime<chrono::Utc>>, i32) =
        sqlx::query_as("SELECT delivered_at, retry_count FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(delivered_at.is_some(), "delivered_at must be set");
    assert_eq!(retry_count, 1);
}

#[actix_web::test]
async fn retry_skips_unsubscribed_rows() {
    let ctx = setup().await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications
            (user_id, template_type, payload, retry_count, is_unsubscribed, created_at)
         VALUES ($1, 'SCHEDULE_CHANGE', '{}'::jsonb, 0, TRUE, NOW() - INTERVAL '1 hour')
         RETURNING id",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/notifications/retry")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    assert_eq!(status_of(&app, req).await, 200);

    let delivered_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT delivered_at FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(delivered_at.is_none(), "unsubscribed rows must not be delivered");
}

#[actix_web::test]
async fn retry_caps_at_max_attempts_and_gives_up() {
    let ctx = setup().await;
    // Row already at max attempts.
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications
            (user_id, template_type, payload, retry_count, created_at)
         VALUES ($1, 'SCHEDULE_CHANGE', '{}'::jsonb, 5, NOW() - INTERVAL '1 day')
         RETURNING id",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/notifications/retry")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["giveup"].as_i64().unwrap() >= 1);

    // retry_count advanced past max; delivered_at still NULL.
    let (delivered_at, retry_count): (Option<chrono::DateTime<chrono::Utc>>, i32) =
        sqlx::query_as("SELECT delivered_at, retry_count FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(delivered_at.is_none());
    assert_eq!(retry_count, 6);
}

#[actix_web::test]
async fn unsubscribe_is_idempotent() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    for _ in 0..3 {
        let req = TestRequest::put()
            .uri("/api/notifications/unsubscribe")
            .insert_header(auth_header(&ctx.tech_a_token))
            .set_json(json!({ "template_type": "CANCELLATION" }))
            .to_request();
        assert_eq!(status_of(&app, req).await, 200);
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_unsubscribes WHERE user_id = $1",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}
