use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, setup, status_of};

#[actix_web::test]
async fn admin_can_create_knowledge_point() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "recipe_id": ctx.recipe_id,
            "title": "Safety check",
            "content": "Wear PPE",
        }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 201);
    assert_eq!(body["title"], "Safety check");
}

#[actix_web::test]
async fn tech_cannot_create_knowledge_point() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "recipe_id": ctx.recipe_id,
            "title": "Attempt",
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 403);
}

#[actix_web::test]
async fn quiz_question_without_options_is_rejected() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "recipe_id": ctx.recipe_id,
            "title": "Bad quiz",
            "quiz_question": "Why?"
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn tech_list_hides_correct_answer() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    // Admin creates with correct answer.
    let kp_req = TestRequest::post()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "recipe_id": ctx.recipe_id,
            "title": "Torque spec",
            "quiz_question": "Correct torque (Nm)?",
            "quiz_options": ["10", "20", "30"],
            "quiz_correct_answer": "20"
        }))
        .to_request();
    let (status, _): (u16, serde_json::Value) = json_of(&app, kp_req).await;
    assert_eq!(status, 201);

    let list_req = TestRequest::get()
        .uri("/api/knowledge-points")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, list_req).await;
    assert_eq!(status, 200);
    let row = &body["data"][0];
    assert_eq!(row["title"], "Torque spec");
    assert!(row.get("quiz_correct_answer").is_none(), "quiz answer must be hidden from techs");
}

#[actix_web::test]
async fn tech_records_correct_quiz_answer_scores_one() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    // Seed a KP with a correct answer directly.
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title, quiz_question, quiz_options, quiz_correct_answer)
         VALUES ($1, 'Q', 'q?', '[\"A\",\"B\"]'::jsonb, 'B') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let req = TestRequest::post()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({
            "knowledge_point_id": kp,
            "quiz_answer": "B",
            "time_spent_seconds": 30
        }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 201);
    assert_eq!(body["quiz_score"].as_f64().unwrap(), 1.0);
    assert_eq!(body["user_id"], ctx.tech_a_id.to_string());
}

#[actix_web::test]
async fn review_bump_increments_count_without_adding_row() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'K') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    // First record.
    let r1 = TestRequest::post()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "knowledge_point_id": kp }))
        .to_request();
    assert_eq!(status_of(&app, r1).await, 201);
    // Review bump.
    let r2 = TestRequest::post()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "knowledge_point_id": kp, "review": true }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, r2).await;
    assert_eq!(status, 200);
    assert_eq!(body["review_count"].as_i64().unwrap(), 1);
    // Only one physical row.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_records WHERE user_id = $1 AND knowledge_point_id = $2",
    )
    .bind(ctx.tech_a_id)
    .bind(kp)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

// -----------------------------------------------------------------------------
// Coverage additions (audit gap: knowledge_points by-step / PUT / DELETE,
// learning_records GET /:id scoping).
// -----------------------------------------------------------------------------

#[actix_web::test]
async fn list_knowledge_by_step_returns_only_rows_for_that_step() {
    let ctx = setup().await;
    let step_a = ctx.step_ids[0];
    let step_b = ctx.step_ids[1];
    // Seed one KP per step so the filter has something to narrow.
    let kp_a: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, step_id, title)
         VALUES ($1, $2, 'KP for A') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .bind(step_a)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    let _: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, step_id, title)
         VALUES ($1, $2, 'KP for B') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .bind(step_b)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/knowledge-points/by-step/{}", step_a))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "only step A's KP should appear");
    assert_eq!(rows[0]["id"], kp_a.to_string());
    assert_eq!(rows[0]["title"], "KP for A");
    // Quiz-safe view: correct answer never exposed on by-step listing.
    assert!(rows[0].get("quiz_correct_answer").is_none());
}

#[actix_web::test]
async fn list_knowledge_by_step_empty_when_no_match() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/knowledge-points/by-step/{}", uuid::Uuid::new_v4()))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], 0);
    assert!(body["data"].as_array().unwrap().is_empty());
}

#[actix_web::test]
async fn admin_updates_knowledge_point_and_body_reflects_new_value() {
    let ctx = setup().await;
    // Seed a KP.
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title, content)
         VALUES ($1, 'Original', 'old content') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/knowledge-points/{}", kp))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "title": "Revised", "content": "new content" }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["id"], kp.to_string());
    assert_eq!(body["title"], "Revised");
    assert_eq!(body["content"], "new content");
}

#[actix_web::test]
async fn update_knowledge_point_404_when_missing() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri(&format!("/api/knowledge-points/{}", uuid::Uuid::new_v4()))
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({ "title": "anything" }))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 404);
    assert_eq!(body["code"], "not_found");
}

#[actix_web::test]
async fn tech_and_super_cannot_update_knowledge_point() {
    let ctx = setup().await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'K') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    for token in [&ctx.tech_a_token, &ctx.super_token] {
        let app = make_service(&ctx).await;
        let req = TestRequest::put()
            .uri(&format!("/api/knowledge-points/{}", kp))
            .insert_header(auth_header(token))
            .set_json(json!({ "title": "hack" }))
            .to_request();
        let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
        assert_eq!(status, 403);
        assert_eq!(body["code"], "forbidden");
    }
}

#[actix_web::test]
async fn admin_can_delete_knowledge_point_and_row_is_gone() {
    let ctx = setup().await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'Doomed') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/knowledge-points/{}", kp))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 204);

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_points WHERE id = $1")
            .bind(kp)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "row must be hard-deleted");
}

#[actix_web::test]
async fn delete_knowledge_point_404_when_missing() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::delete()
        .uri(&format!("/api/knowledge-points/{}", uuid::Uuid::new_v4()))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn non_admin_cannot_delete_knowledge_point() {
    let ctx = setup().await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'Survives') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    for token in [&ctx.tech_a_token, &ctx.super_token] {
        let app = make_service(&ctx).await;
        let req = TestRequest::delete()
            .uri(&format!("/api/knowledge-points/{}", kp))
            .insert_header(auth_header(token))
            .to_request();
        assert_eq!(status_of(&app, req).await, 403);
    }
    // Still present.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_points WHERE id = $1")
        .bind(kp)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[actix_web::test]
async fn get_learning_record_by_id_admin_sees_any() {
    let ctx = setup().await;
    // Tech A records so we have something to fetch.
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'K') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    let rec_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO learning_records
            (user_id, knowledge_point_id, time_spent_seconds, review_count, completed_at)
         VALUES ($1, $2, 90, 0, NOW()) RETURNING id",
    )
    .bind(ctx.tech_a_id)
    .bind(kp)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/learning-records/{}", rec_id))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["id"], rec_id.to_string());
    assert_eq!(body["user_id"], ctx.tech_a_id.to_string());
    assert_eq!(body["knowledge_point_id"], kp.to_string());
}

#[actix_web::test]
async fn get_learning_record_by_id_tech_sees_own() {
    let ctx = setup().await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'K') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    let rec_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO learning_records
            (user_id, knowledge_point_id, review_count, completed_at)
         VALUES ($1, $2, 0, NOW()) RETURNING id",
    )
    .bind(ctx.tech_a_id)
    .bind(kp)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/learning-records/{}", rec_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["user_id"], ctx.tech_a_id.to_string());
}

#[actix_web::test]
async fn get_learning_record_tech_cannot_see_other_tech_record() {
    let ctx = setup().await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'K') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    // Record owned by tech_b.
    let rec_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO learning_records
            (user_id, knowledge_point_id, review_count, completed_at)
         VALUES ($1, $2, 0, NOW()) RETURNING id",
    )
    .bind(ctx.tech_b_id)
    .bind(kp)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/learning-records/{}", rec_id))
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    // Scope miss returns 404 (not 403) to avoid enumeration.
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 404);
    assert_eq!(body["code"], "not_found");
}

#[actix_web::test]
async fn get_learning_record_404_for_unknown_id() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri(&format!("/api/learning-records/{}", uuid::Uuid::new_v4()))
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    assert_eq!(status_of(&app, req).await, 404);
}

#[actix_web::test]
async fn tech_list_scoped_to_own_records() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let kp: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_points (recipe_id, title) VALUES ($1, 'K') RETURNING id",
    )
    .bind(ctx.recipe_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    // Tech A and Tech B each record.
    for token in [&ctx.tech_a_token, &ctx.tech_b_token] {
        let r = TestRequest::post()
            .uri("/api/learning-records")
            .insert_header(auth_header(token))
            .set_json(json!({ "knowledge_point_id": kp }))
            .to_request();
        assert_eq!(status_of(&app, r).await, 201);
    }
    // Tech A sees only own.
    let list = TestRequest::get()
        .uri("/api/learning-records")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, list).await;
    assert_eq!(status, 200);
    let rows = body["data"].as_array().unwrap();
    for r in rows {
        assert_eq!(r["user_id"], ctx.tech_a_id.to_string());
    }
}
