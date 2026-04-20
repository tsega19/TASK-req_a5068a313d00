//! SLA timeout alert coverage (PRD §7). The worker scans work orders whose
//! elapsed fraction of their SLA window crosses a configured threshold and
//! enqueues deduped notifications.

use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, raw_of, setup, status_of};

#[actix_web::test]
async fn sla_scan_emits_notification_when_deadline_crossed() {
    let ctx = setup().await;

    // Push WO-A's created_at backwards so the 1.00 threshold is clearly
    // crossed regardless of the seeded "8 hours ahead" sla_deadline.
    sqlx::query(
        "UPDATE work_orders
         SET created_at = NOW() - INTERVAL '10 days',
             sla_deadline = NOW() - INTERVAL '1 day'
         WHERE id = $1",
    )
    .bind(ctx.wo_a_id)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/sla/scan")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["alerts_emitted"].as_i64().unwrap() >= 1);

    let recipients: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE template_type = 'SCHEDULE_CHANGE'
           AND payload @> $1::jsonb",
    )
    .bind(json!({ "sla_alert": { "work_order_id": ctx.wo_a_id } }))
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(recipients >= 1, "assigned tech should have been notified");
}

#[actix_web::test]
async fn sla_scan_deduplicates_repeat_runs() {
    let ctx = setup().await;
    sqlx::query(
        "UPDATE work_orders
         SET created_at = NOW() - INTERVAL '10 days',
             sla_deadline = NOW() - INTERVAL '1 day'
         WHERE id = $1",
    )
    .bind(ctx.wo_a_id)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app1 = super::common::make_service(&ctx).await;
    let r1 = TestRequest::post()
        .uri("/api/admin/sla/scan")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (_, body1) = raw_of(&app1, r1).await;
    let first_emitted = body1["alerts_emitted"].as_i64().unwrap();
    assert!(first_emitted >= 1);

    let app2 = super::common::make_service(&ctx).await;
    let r2 = TestRequest::post()
        .uri("/api/admin/sla/scan")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (_, body2) = raw_of(&app2, r2).await;
    assert_eq!(
        body2["alerts_emitted"].as_i64().unwrap(),
        0,
        "second scan must not re-emit already-alerted thresholds"
    );
    assert!(body2["deduped"].as_i64().unwrap() >= first_emitted);
}

#[actix_web::test]
async fn sla_scan_requires_admin() {
    let ctx = setup().await;
    let app = super::common::make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/sla/scan")
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}
