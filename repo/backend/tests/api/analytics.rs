use actix_web::test::{self, TestRequest};

use super::common::{auth_header, json_of, make_service, setup, status_of};

async fn seed_learning_record(pool: &sqlx::PgPool, user_id: uuid::Uuid, recipe: uuid::Uuid) {
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title, content) VALUES ($1, 'K1', 'c')
         RETURNING id",
    )
    .bind(recipe)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_records (user_id, knowledge_point_id, quiz_score,
            time_spent_seconds, review_count, completed_at)
         VALUES ($1, $2, 0.85, 120, 1, NOW())",
    )
    .bind(user_id)
    .bind(kp)
    .execute(pool)
    .await
    .unwrap();
}

#[actix_web::test]
async fn learning_admin_sees_all_rows() {
    let ctx = setup().await;
    seed_learning_record(&ctx.pool, ctx.tech_a_id, ctx.recipe_id).await;
    seed_learning_record(&ctx.pool, ctx.tech_b_id, ctx.recipe_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/learning")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    // All users (admin, super, tech_a, tech_b) appear; two have completions.
    let completions: i64 = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["completion_count"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(completions, 2);
}

#[actix_web::test]
async fn learning_tech_sees_only_own_row() {
    let ctx = setup().await;
    seed_learning_record(&ctx.pool, ctx.tech_a_id, ctx.recipe_id).await;
    seed_learning_record(&ctx.pool, ctx.tech_b_id, ctx.recipe_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/learning")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["user_id"], ctx.tech_a_id.to_string());
}

#[actix_web::test]
async fn learning_rejects_bad_date_format() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/learning?from=2026-01-01")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn learning_csv_has_watermark_footer() {
    let ctx = setup().await;
    seed_learning_record(&ctx.pool, ctx.tech_a_id, ctx.recipe_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/learning/export-csv")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status().as_u16(), 200);
    let ct = resp
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let cd = resp
        .headers()
        .get("Content-Disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = test::read_body(resp).await;
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(ct.starts_with("text/csv"));
    assert!(cd.contains("learning-analytics.csv"));
    assert!(text.contains("user_id,username,role"));
    assert!(text.contains("# Exported by: admin at "));
}

// -----------------------------------------------------------------------------
// Pipeline: records written via /api/learning-records are visible in analytics.
// -----------------------------------------------------------------------------
#[actix_web::test]
async fn analytics_reflects_records_written_via_capture_endpoint() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;

    // Admin authors a knowledge point (with a quiz) tied to the recipe.
    let kp_req = TestRequest::post()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(serde_json::json!({
            "recipe_id": ctx.recipe_id,
            "title": "Refrigerant handling",
            "content": "EPA 608 basics",
            "quiz_question": "Is it safe to vent refrigerant?",
            "quiz_options": ["YES", "NO"],
            "quiz_correct_answer": "NO"
        }))
        .to_request();
    let (status, kp_body): (u16, serde_json::Value) =
        json_of(&app, kp_req).await;
    assert_eq!(status, 201);
    let kp_id = kp_body["id"].as_str().unwrap();

    // Tech A records a correct answer.
    let rec_req = TestRequest::post()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(serde_json::json!({
            "knowledge_point_id": kp_id,
            "work_order_id": ctx.wo_a_id,
            "quiz_answer": "NO",
            "time_spent_seconds": 90
        }))
        .to_request();
    assert_eq!(status_of(&app, rec_req).await, 201);

    // Analytics now reflects the capture for tech_a.
    let a_req = TestRequest::get()
        .uri("/api/analytics/learning")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, a_req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    let tech_a_row = rows
        .iter()
        .find(|r| r["user_id"] == ctx.tech_a_id.to_string())
        .expect("tech_a row present");
    assert_eq!(tech_a_row["completion_count"].as_i64().unwrap(), 1);
    assert_eq!(tech_a_row["quiz_avg"].as_f64().unwrap(), 1.0);
}

#[actix_web::test]
async fn analytics_branch_filter_narrows_to_single_branch() {
    let ctx = setup().await;
    seed_learning_record(&ctx.pool, ctx.tech_a_id, ctx.recipe_id).await;
    seed_learning_record(&ctx.pool, ctx.tech_b_id, ctx.recipe_id).await;
    let app = make_service(&ctx).await;
    let url = format!("/api/analytics/learning?branch={}", ctx.branch_a_id);
    let req = TestRequest::get()
        .uri(&url)
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    // Only branch_a users appear; total completion count is 1 (tech_a).
    let rows = body["data"].as_array().unwrap();
    let completions: i64 = rows
        .iter()
        .map(|r| r["completion_count"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(completions, 1);
    // No branch_b users in the result set.
    assert!(rows
        .iter()
        .all(|r| r["user_id"] != ctx.tech_b_id.to_string()));
}

#[actix_web::test]
async fn analytics_date_range_filter_excludes_out_of_window_records() {
    let ctx = setup().await;
    // Record completed today.
    seed_learning_record(&ctx.pool, ctx.tech_a_id, ctx.recipe_id).await;
    // Backdate it one year.
    sqlx::query(
        "UPDATE learning_records SET completed_at = NOW() - INTERVAL '365 days'
         WHERE user_id = $1",
    )
    .bind(ctx.tech_a_id)
    .execute(&ctx.pool)
    .await
    .unwrap();
    let app = make_service(&ctx).await;
    // Query for a window that excludes the old record.
    let today = chrono::Utc::now().format("%m/%d/%Y").to_string();
    let url = format!("/api/analytics/learning?from={}&to={}", today, today);
    let req = TestRequest::get()
        .uri(&url)
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    let completions: i64 = rows
        .iter()
        .map(|r| r["completion_count"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(completions, 0);
}

#[actix_web::test]
async fn analytics_role_filter_limits_to_requested_role() {
    let ctx = setup().await;
    seed_learning_record(&ctx.pool, ctx.tech_a_id, ctx.recipe_id).await;
    seed_learning_record(&ctx.pool, ctx.super_id, ctx.recipe_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/learning?role=TECH")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    for r in rows {
        assert_eq!(r["role"], "TECH");
    }
}

// -----------------------------------------------------------------------------
// Trend endpoints: /api/analytics/trends/{knowledge-points,units,workflows}.
// Guard against the newly-added trend builder regressing silently.
// -----------------------------------------------------------------------------

/// Seeds one pass (quiz_score=1.0) and one fail (quiz_score=0.0) on the same
/// knowledge point for the given user — enough to verify both completion
/// counting and the completion_rate metric end-to-end.
async fn seed_pass_and_fail(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    recipe: uuid::Uuid,
    work_order_id: uuid::Uuid,
) -> uuid::Uuid {
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'Trend KP') RETURNING id",
    )
    .bind(recipe)
    .fetch_one(pool)
    .await
    .unwrap();
    // One pass, one fail — completion_rate should resolve to 0.5.
    for score in [1.0f64, 0.0f64] {
        sqlx::query(
            "INSERT INTO learning_records (user_id, knowledge_point_id, work_order_id,
                quiz_score, time_spent_seconds, review_count, completed_at)
             VALUES ($1, $2, $3, $4, 60, 0, NOW())",
        )
        .bind(user_id)
        .bind(kp)
        .bind(work_order_id)
        .bind(score)
        .execute(pool)
        .await
        .unwrap();
    }
    kp
}

#[actix_web::test]
async fn trends_knowledge_points_reports_completion_rate() {
    let ctx = setup().await;
    let kp = seed_pass_and_fail(&ctx.pool, ctx.tech_a_id, ctx.recipe_id, ctx.wo_a_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/trends/knowledge-points")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let row = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["group_id"] == kp.to_string())
        .expect("seeded KP must appear in trend output");
    assert_eq!(row["attempt_count"].as_i64().unwrap(), 2);
    assert_eq!(row["completion_count"].as_i64().unwrap(), 1);
    let rate = row["completion_rate"].as_f64().unwrap();
    assert!((rate - 0.5).abs() < 1e-9, "completion_rate should be 0.5, got {}", rate);
    assert_eq!(row["group_label"].as_str().unwrap(), "Trend KP");
}

#[actix_web::test]
async fn trends_units_groups_by_recipe() {
    let ctx = setup().await;
    let _ = seed_pass_and_fail(&ctx.pool, ctx.tech_a_id, ctx.recipe_id, ctx.wo_a_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/trends/units")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let row = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["group_id"] == ctx.recipe_id.to_string())
        .expect("seed recipe must appear as a unit group");
    assert!(row["attempt_count"].as_i64().unwrap() >= 2);
    assert!(row["completion_count"].as_i64().unwrap() >= 1);
    assert_eq!(row["group_label"].as_str().unwrap(), "Refrigeration Service");
}

#[actix_web::test]
async fn trends_workflows_groups_by_work_order() {
    let ctx = setup().await;
    let _ = seed_pass_and_fail(&ctx.pool, ctx.tech_a_id, ctx.recipe_id, ctx.wo_a_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/trends/workflows")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let row = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["group_id"] == ctx.wo_a_id.to_string())
        .expect("seed work_order must appear as a workflow group");
    assert_eq!(row["attempt_count"].as_i64().unwrap(), 2);
    assert_eq!(row["completion_count"].as_i64().unwrap(), 1);
}

#[actix_web::test]
async fn trends_respect_tech_scope() {
    let ctx = setup().await;
    // Seed records for both tech_a and tech_b so a wrong scope would pick up
    // foreign rows.
    let _ = seed_pass_and_fail(&ctx.pool, ctx.tech_a_id, ctx.recipe_id, ctx.wo_a_id).await;
    let _ = seed_pass_and_fail(&ctx.pool, ctx.tech_b_id, ctx.recipe_id, ctx.wo_b_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/trends/workflows")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    // TECH must see only their own work_order, never the other branch's.
    let ids: Vec<String> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["group_id"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(ids.contains(&ctx.wo_a_id.to_string()));
    assert!(!ids.contains(&ctx.wo_b_id.to_string()), "TECH must not see other tech's WO");
}

#[actix_web::test]
async fn trends_rejects_invalid_bucket() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/trends/knowledge-points?bucket=fortnight")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn trends_week_bucket_returns_bucket_start() {
    let ctx = setup().await;
    let _ = seed_pass_and_fail(&ctx.pool, ctx.tech_a_id, ctx.recipe_id, ctx.wo_a_id).await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/analytics/trends/knowledge-points?bucket=week")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let row = &body["data"].as_array().unwrap()[0];
    // With a bucket set, bucket_start must be populated (not null).
    assert!(row["bucket_start"].is_string());
}
