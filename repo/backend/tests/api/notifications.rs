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
async fn retry_does_not_deliver_simulated_failure() {
    // Rows whose payload carries the `_simulate_failure` marker must NOT be
    // marked delivered — the retry worker bumps `retry_count` so the DB
    // reflects that an attempt was made, but `delivered_at` stays NULL.
    let ctx = setup().await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications
            (user_id, template_type, payload, retry_count, created_at)
         VALUES ($1, 'SCHEDULE_CHANGE',
                 '{\"_simulate_failure\": true}'::jsonb,
                 0, NOW() - INTERVAL '10 seconds')
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
    assert!(
        body["failed_again"].as_i64().unwrap() >= 1,
        "retry worker must record the simulated failure"
    );

    let (delivered_at, retry_count): (Option<chrono::DateTime<chrono::Utc>>, i32) =
        sqlx::query_as("SELECT delivered_at, retry_count FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(delivered_at.is_none(), "simulated failure must leave delivered_at NULL");
    assert_eq!(retry_count, 1, "retry_count must still advance on failure");
}

#[actix_web::test]
async fn retry_converges_after_simulated_failure_count_exhausts() {
    // `_simulate_failure_count: 1` — first retry attempt fails, second succeeds.
    let ctx = setup().await;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO notifications
            (user_id, template_type, payload, retry_count, created_at)
         VALUES ($1, 'SCHEDULE_CHANGE',
                 '{\"_simulate_failure_count\": 1}'::jsonb,
                 0, NOW() - INTERVAL '30 seconds')
         RETURNING id",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    // First tick: retry_count 0 -> 1, fails (attempt 1 <= count 1).
    let req = TestRequest::post()
        .uri("/api/admin/notifications/retry")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    let (_, _): (u16, serde_json::Value) = json_of(&app, req).await;

    // Second tick: retry_count 1 -> 2, succeeds (attempt 2 > count 1).
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::post()
        .uri("/api/admin/notifications/retry")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    let (status2, _): (u16, serde_json::Value) = json_of(&app2, req2).await;
    assert_eq!(status2, 200);

    let (delivered_at, retry_count): (Option<chrono::DateTime<chrono::Utc>>, i32) =
        sqlx::query_as("SELECT delivered_at, retry_count FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(delivered_at.is_some(), "second retry must deliver");
    assert_eq!(retry_count, 2);
}

#[actix_web::test]
async fn rate_limited_row_becomes_eligible_after_window() {
    // Seed `max + 1` delivered rows for the user but backdate them past the
    // rolling 1-hour window. A fresh pending row inserted now should no
    // longer be blocked by the rate-limit check in the retry worker.
    let ctx = setup().await;
    let max = ctx.cfg.business.max_notifications_per_hour as i32;
    for _ in 0..max {
        sqlx::query(
            "INSERT INTO notifications
                (user_id, template_type, payload, retry_count,
                 delivered_at, created_at)
             VALUES ($1, 'SCHEDULE_CHANGE', '{}'::jsonb, 1,
                     NOW() - INTERVAL '2 hours',
                     NOW() - INTERVAL '2 hours')",
        )
        .bind(ctx.tech_a_id)
        .execute(&ctx.pool)
        .await
        .unwrap();
    }
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
    let (status, _body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);

    let delivered_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT delivered_at FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(
        delivered_at.is_some(),
        "row must be delivered once the rate-limit window rolls forward"
    );
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

// -----------------------------------------------------------------------------
// Domain-triggered template emission (audit-2 Medium #2). Every template from
// the enum must have a real call site; these tests assert each one actually
// fires when the triggering domain action runs.
// -----------------------------------------------------------------------------

#[actix_web::test]
async fn admin_user_create_emits_signup_success_notification() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/users")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "username": "onboarded_tech",
            "password": "a-sufficiently-long-password",
            "role": "TECH",
            "branch_id": ctx.branch_a_id,
        }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 201);
    let new_user_id = body["id"].as_str().unwrap().parse::<Uuid>().unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE user_id = $1 AND template_type = 'SIGNUP_SUCCESS'",
    )
    .bind(new_user_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "new user must receive a SIGNUP_SUCCESS notification");
}

#[actix_web::test]
async fn work_order_cancel_transition_emits_cancellation_to_assigned_tech() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    // WO-A is assigned to tech_a. An admin cancels it — the tech must be
    // notified, not the admin who triggered the action.
    let req = TestRequest::put()
        .uri(&format!("/api/work-orders/{}/state", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "to_state": "Canceled",
            "notes": "customer rescheduled",
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 200);

    let tech_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE user_id = $1 AND template_type = 'CANCELLATION'",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(tech_count, 1);
    // Admin (the actor) must NOT receive their own cancellation notification.
    let admin_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE user_id = $1 AND template_type = 'CANCELLATION'",
    )
    .bind(ctx.admin_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(admin_count, 0);
}

#[actix_web::test]
async fn graded_learning_record_emits_review_result_to_learner() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    // Author a knowledge point with a quiz first.
    let kp_req = TestRequest::post()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "recipe_id": ctx.recipe_id,
            "title": "Notify-on-review KP",
            "quiz_question": "Is it safe?",
            "quiz_options": ["YES", "NO"],
            "quiz_correct_answer": "NO",
        }))
        .to_request();
    let (s, body): (u16, serde_json::Value) = json_of(&app, kp_req).await;
    assert_eq!(s, 201);
    let kp_id = body["id"].as_str().unwrap().parse::<Uuid>().unwrap();

    // Tech submits an answer — graded, so REVIEW_RESULT must fire.
    let app2 = make_service(&ctx).await;
    let submit = TestRequest::post()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "knowledge_point_id": kp_id,
            "quiz_answer": "NO",
        }))
        .to_request();
    assert_eq!(status_of(&app2, submit).await, 201);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE user_id = $1 AND template_type = 'REVIEW_RESULT'",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[actix_web::test]
async fn ungraded_learning_record_does_not_emit_review_result() {
    // If the KP has no quiz, there is no score to review — no notification.
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let kp_id: Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'Reading-only KP') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    let submit = TestRequest::post()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "knowledge_point_id": kp_id,
            "time_spent_seconds": 60,
        }))
        .to_request();
    assert_eq!(status_of(&app, submit).await, 201);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE user_id = $1 AND template_type = 'REVIEW_RESULT'",
    )
    .bind(ctx.tech_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "ungraded record must not emit REVIEW_RESULT");
}
