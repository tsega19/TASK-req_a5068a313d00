use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, setup, status_of};

#[actix_web::test]
async fn changes_endpoint_returns_rows_since_cursor() {
    let ctx = setup().await;
    // Seed a sync_log row.
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation)
         VALUES ('work_orders', $1, 'UPDATE')",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&ctx.pool)
    .await
    .unwrap();
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/sync/changes")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body["count"].as_i64().unwrap() >= 1);
    assert!(body["data"].as_array().unwrap().iter().any(|r| r["entity_table"] == "work_orders"));
}

#[actix_web::test]
async fn changes_entity_filter_applies() {
    let ctx = setup().await;
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation)
         VALUES ('tip_cards', $1, 'UPDATE')",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&ctx.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation)
         VALUES ('recipes', $1, 'UPDATE')",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&ctx.pool)
    .await
    .unwrap();
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/sync/changes?entity=tip_cards")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    for r in body["data"].as_array().unwrap() {
        assert_eq!(r["entity_table"], "tip_cards");
    }
}

#[actix_web::test]
async fn changes_invalid_cursor_returns_400() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/sync/changes?since=not-a-date")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn changes_tech_only_sees_own_work_order_rows() {
    // Tech A is assigned to wo_a; Tech B to wo_b. Seed a sync_log row for
    // each work order and verify Tech A only ever sees wo_a's id in the
    // response — no metadata leak of wo_b's uuid via sync_log.
    let ctx = setup().await;
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation)
         VALUES ('work_orders', $1, 'UPDATE'),
                ('work_orders', $2, 'UPDATE')",
    )
    .bind(ctx.wo_a_id)
    .bind(ctx.wo_b_id)
    .execute(&ctx.pool)
    .await
    .unwrap();
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/sync/changes")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let ids: Vec<String> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["entity_table"] == "work_orders")
        .map(|r| r["entity_id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&ctx.wo_a_id.to_string()));
    assert!(
        !ids.contains(&ctx.wo_b_id.to_string()),
        "Tech A must not see wo_b's entity_id"
    );
}

#[actix_web::test]
async fn changes_super_scoped_to_branch() {
    // super_a is bound to branch A. Seeding sync_log for wo_a (branch A)
    // and wo_b (branch B) — super_a must only see wo_a.
    let ctx = setup().await;
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation)
         VALUES ('work_orders', $1, 'UPDATE'),
                ('work_orders', $2, 'UPDATE')",
    )
    .bind(ctx.wo_a_id)
    .bind(ctx.wo_b_id)
    .execute(&ctx.pool)
    .await
    .unwrap();
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/sync/changes")
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let ids: Vec<String> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["entity_table"] == "work_orders")
        .map(|r| r["entity_id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&ctx.wo_a_id.to_string()));
    assert!(
        !ids.contains(&ctx.wo_b_id.to_string()),
        "branch-scoped super must not see wo_b's id"
    );
}

#[actix_web::test]
async fn soft_delete_propagates_as_delete_sync_log() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/work-orders/{}", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 204);
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log
         WHERE entity_table = 'work_orders'
           AND entity_id = $1
           AND operation = 'DELETE'",
    )
    .bind(ctx.wo_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(n >= 1, "DELETE tombstone expected in sync_log");
}

#[actix_web::test]
async fn push_work_order_delete_requires_admin() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/sync/work-orders/{}/delete", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.super_token))
        .set_json(json!({}))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

// -----------------------------------------------------------------------------
// Coverage additions (audit gap): POST /api/sync/step-progress, GET
// /api/sync/conflicts, happy-path POST /api/sync/work-orders/:id/delete.
// -----------------------------------------------------------------------------

#[actix_web::test]
async fn post_step_progress_inserts_when_no_local_row() {
    let ctx = setup().await;
    let step = ctx.step_ids[0];
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/sync/step-progress")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "work_order_id": ctx.wo_a_id,
            "step_id": step,
            "status": "InProgress",
            "notes": "from offline replica",
            "timer_state_snapshot": null,
            "version": 1,
            "updated_at": "2026-04-18T00:00:00Z"
        }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "applied");
    assert_eq!(body["conflict"], false);

    // Verify the row actually landed in the DB with the pushed payload.
    let row: (String, Option<String>, i32) = sqlx::query_as(
        "SELECT status::text, notes, version FROM job_step_progress
         WHERE work_order_id = $1 AND step_id = $2",
    )
    .bind(ctx.wo_a_id)
    .bind(step)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(row.0, "InProgress");
    assert_eq!(row.1.as_deref(), Some("from offline replica"));
    assert_eq!(row.2, 1);
}

#[actix_web::test]
async fn post_step_progress_rejects_older_version() {
    let ctx = setup().await;
    let step = ctx.step_ids[0];
    // Seed a local row already at version 5.
    sqlx::query(
        "INSERT INTO job_step_progress
            (work_order_id, step_id, status, version)
         VALUES ($1, $2, 'InProgress', 5)",
    )
    .bind(ctx.wo_a_id)
    .bind(step)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/sync/step-progress")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "work_order_id": ctx.wo_a_id,
            "step_id": step,
            "status": "Paused",
            "notes": null,
            "timer_state_snapshot": null,
            "version": 3,
            "updated_at": "2026-04-18T00:00:00Z"
        }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "rejected_older");
    assert_eq!(body["conflict"], false);

    // Local row should be untouched.
    let (cur_status, cur_version): (String, i32) = sqlx::query_as(
        "SELECT status::text, version FROM job_step_progress
         WHERE work_order_id = $1 AND step_id = $2",
    )
    .bind(ctx.wo_a_id)
    .bind(step)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(cur_status, "InProgress");
    assert_eq!(cur_version, 5);
}

#[actix_web::test]
async fn post_step_progress_flags_conflict_on_equal_version_equal_timestamp_different_payload() {
    // With the PRD §8 timestamp-priority rule in place, only a *strictly equal*
    // updated_at (plus equal version, divergent payload) is ambiguous enough
    // to escalate. Pin both sides to the same instant to hit that branch.
    let ctx = setup().await;
    let step = ctx.step_ids[0];
    let pinned_ts = "2026-04-18T00:00:00Z";
    sqlx::query(
        "INSERT INTO job_step_progress
            (work_order_id, step_id, status, notes, version, updated_at)
         VALUES ($1, $2, 'InProgress', 'local note', 7, $3::timestamptz)",
    )
    .bind(ctx.wo_a_id)
    .bind(step)
    .bind(pinned_ts)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/sync/step-progress")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "work_order_id": ctx.wo_a_id,
            "step_id": step,
            "status": "Paused",                // differs from local
            "notes": "conflicting note",
            "timer_state_snapshot": null,
            "version": 7,                      // same version
            "updated_at": pinned_ts            // same timestamp → genuine tie
        }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["outcome"], "conflict");
    assert_eq!(body["conflict"], true);

    // sync_log has a flagged row awaiting SUPER review. `entity_id` is the
    // `job_step_progress.id` (see sync::merge::log_sync comment), so join
    // through to match on the (work_order_id, step_id) pair the test knows.
    let flagged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log s
         JOIN job_step_progress p ON p.id = s.entity_id
         WHERE s.entity_table = 'job_step_progress'
           AND p.work_order_id = $1
           AND p.step_id = $2
           AND s.conflict_flagged = TRUE
           AND s.conflict_resolved_by IS NULL",
    )
    .bind(ctx.wo_a_id)
    .bind(step)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(flagged >= 1, "conflict row must be recorded");
}

#[actix_web::test]
async fn post_step_progress_other_techs_work_order_returns_404() {
    // tech_a is NOT assigned to wo_b — load_visible hides the work order, so
    // the sync push returns 404, never 403, to avoid enumeration.
    let ctx = setup().await;
    let step = ctx.step_ids[0];
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/sync/step-progress")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "work_order_id": ctx.wo_b_id,
            "step_id": step,
            "status": "InProgress",
            "notes": null,
            "timer_state_snapshot": null,
            "version": 1,
            "updated_at": "2026-04-18T00:00:00Z"
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn conflicts_list_super_happy_and_tech_is_403() {
    let ctx = setup().await;
    // Seed one unresolved, one resolved.
    sqlx::query(
        "INSERT INTO sync_log
            (entity_table, entity_id, operation, conflict_flagged)
         VALUES ('job_step_progress', $1, 'UPDATE', TRUE)",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&ctx.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sync_log
            (entity_table, entity_id, operation, conflict_flagged, conflict_resolved_by)
         VALUES ('job_step_progress', $1, 'UPDATE', TRUE, $2)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(ctx.admin_id)
    .execute(&ctx.pool)
    .await
    .unwrap();

    // SUPER sees only the unresolved row.
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/sync/conflicts")
        .insert_header(auth_header(&ctx.super_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "resolved rows must be excluded");
    assert_eq!(rows[0]["entity_table"], "job_step_progress");
    assert!(rows[0]["id"].as_str().is_some());
    assert!(rows[0]["synced_at"].as_str().is_some());

    // TECH is forbidden.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri("/api/sync/conflicts")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status2, body2): (u16, serde_json::Value) = json_of(&app2, req2).await;
    assert_eq!(status2, 403);
    assert_eq!(body2["code"], "forbidden");
}

#[actix_web::test]
async fn push_work_order_delete_admin_happy_path_writes_tombstone() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/sync/work-orders/{}/delete", ctx.wo_a_id))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["entity_table"], "work_orders");
    assert_eq!(body["entity_id"], ctx.wo_a_id.to_string());

    // deleted_at is set on the row.
    let dt: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM work_orders WHERE id = $1")
            .bind(ctx.wo_a_id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert!(dt.is_some(), "work order must be soft-deleted");

    // sync_log gets a DELETE tombstone so replicas converge.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log
         WHERE entity_table = 'work_orders'
           AND entity_id = $1
           AND operation = 'DELETE'",
    )
    .bind(ctx.wo_a_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(n >= 1, "DELETE tombstone must be recorded");
}

#[actix_web::test]
async fn push_work_order_delete_404_for_missing_id() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/sync/work-orders/{}/delete", uuid::Uuid::new_v4()))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({}))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn resolve_conflict_integration() {
    let ctx = setup().await;
    // Seed a flagged conflict row.
    let conflict_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO sync_log (entity_table, entity_id, operation, conflict_flagged)
         VALUES ('job_step_progress', $1, 'UPDATE', TRUE) RETURNING id",
    )
    .bind(uuid::Uuid::new_v4())
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri(&format!("/api/sync/conflicts/{}/resolve", conflict_id))
        .insert_header(auth_header(&ctx.super_token))
        .set_json(json!({ "acknowledged": true }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["resolved"], true);
    let resolver: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT conflict_resolved_by FROM sync_log WHERE id = $1",
    )
    .bind(conflict_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(resolver, Some(ctx.super_id));
}
