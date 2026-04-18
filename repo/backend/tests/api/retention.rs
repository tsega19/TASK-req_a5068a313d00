use actix_web::test::TestRequest;

use super::common::{auth_header, json_of, make_service, setup, status_of};

#[actix_web::test]
async fn prune_removes_work_orders_past_retention() {
    let ctx = setup().await;
    // Backdate wo_a's deleted_at beyond 90 days.
    sqlx::query(
        "UPDATE work_orders SET deleted_at = NOW() - INTERVAL '200 days'
         WHERE id = $1",
    )
    .bind(ctx.wo_a_id)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/retention/prune")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["work_orders_pruned"].as_i64().unwrap() >= 1);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_orders WHERE id = $1")
        .bind(ctx.wo_a_id)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[actix_web::test]
async fn prune_preserves_recent_soft_deletes() {
    let ctx = setup().await;
    // wo_a was just soft-deleted.
    sqlx::query("UPDATE work_orders SET deleted_at = NOW() WHERE id = $1")
        .bind(ctx.wo_a_id)
        .execute(&ctx.pool)
        .await
        .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/retention/prune")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, _): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);

    // Still there — under the retention window.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_orders WHERE id = $1")
        .bind(ctx.wo_a_id)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[actix_web::test]
async fn prune_endpoint_requires_admin() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/admin/retention/prune")
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}
