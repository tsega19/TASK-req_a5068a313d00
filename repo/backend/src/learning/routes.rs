//! Learning pipeline: knowledge points authoring + quiz delivery + learning
//! record capture (PRD §5 / §7 "Knowledge / Quiz").
//!
//! RBAC:
//!   - Knowledge point CRUD: ADMIN only (authoring).
//!   - Knowledge point read + quiz delivery: any authenticated user.
//!   - Learning records write: caller writes only for themselves (TECH primary;
//!     SUPER/ADMIN may also record). Enforced by tying `user_id` to the JWT.
//!   - Learning records read: TECH sees own; SUPER sees branch team; ADMIN sees all.

use actix_web::{delete, get, post, put, web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::config::AppConfig;
use crate::errors::ApiError;
use crate::log_info;
use crate::middleware::rbac::{require_any_role, require_branch_scope, require_role, AuthedUser};
use crate::processing_log;

const MODULE: &str = "learning";

// -----------------------------------------------------------------------------
// Models
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KnowledgePoint {
    pub id: Uuid,
    pub recipe_id: Uuid,
    pub step_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub quiz_question: Option<String>,
    pub quiz_options: Option<serde_json::Value>,
    pub quiz_correct_answer: Option<String>,
}

/// Public quiz view — omits the correct answer so techs can't cheat.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KnowledgeQuizView {
    pub id: Uuid,
    pub recipe_id: Uuid,
    pub step_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub quiz_question: Option<String>,
    pub quiz_options: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKnowledgePoint {
    pub recipe_id: Uuid,
    pub step_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub quiz_question: Option<String>,
    pub quiz_options: Option<serde_json::Value>,
    pub quiz_correct_answer: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateKnowledgePoint {
    pub title: Option<String>,
    pub content: Option<String>,
    pub quiz_question: Option<String>,
    pub quiz_options: Option<serde_json::Value>,
    pub quiz_correct_answer: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LearningRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub knowledge_point_id: Uuid,
    pub work_order_id: Option<Uuid>,
    pub quiz_score: Option<f64>,
    pub time_spent_seconds: Option<i32>,
    pub review_count: i32,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct RecordLearningBody {
    pub knowledge_point_id: Uuid,
    pub work_order_id: Option<Uuid>,
    /// Technician-supplied answer; graded against `quiz_correct_answer`.
    pub quiz_answer: Option<String>,
    pub time_spent_seconds: Option<i32>,
    /// If true, increments `review_count` for an existing record instead of
    /// inserting a fresh one. The existing completion score is preserved.
    #[serde(default)]
    pub review: bool,
}

// -----------------------------------------------------------------------------
// Knowledge-point routes
// -----------------------------------------------------------------------------

#[get("")]
pub async fn list_knowledge_points(
    user: AuthedUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    // Public to all authed roles. Admins see full KP (with correct answer);
    // others get the quiz-safe view.
    if matches!(user.role(), Role::Admin) {
        let rows = sqlx::query_as::<_, KnowledgePoint>(
            "SELECT id, recipe_id, step_id, title, content, quiz_question,
                    quiz_options, quiz_correct_answer
             FROM knowledge_points ORDER BY title ASC",
        )
        .fetch_all(pool.get_ref())
        .await?;
        log_info!(MODULE, "kp_list", "admin={} count={}", user.user_id(), rows.len());
        Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
    } else {
        let rows = sqlx::query_as::<_, KnowledgeQuizView>(
            "SELECT id, recipe_id, step_id, title, content, quiz_question, quiz_options
             FROM knowledge_points ORDER BY title ASC",
        )
        .fetch_all(pool.get_ref())
        .await?;
        log_info!(MODULE, "kp_list", "user={} count={}", user.user_id(), rows.len());
        Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
    }
}

#[get("/by-step/{step_id}")]
pub async fn list_knowledge_by_step(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let step_id = path.into_inner();
    let rows = sqlx::query_as::<_, KnowledgeQuizView>(
        "SELECT id, recipe_id, step_id, title, content, quiz_question, quiz_options
         FROM knowledge_points WHERE step_id = $1 ORDER BY title ASC",
    )
    .bind(step_id)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "kp_by_step", "user={} step={} count={}", user.user_id(), step_id, rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

#[post("")]
pub async fn create_knowledge_point(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    body: web::Json<CreateKnowledgePoint>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let req = body.into_inner();
    if req.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title required".into()));
    }
    // If a quiz question is provided, require options + correct answer so
    // scoring can actually occur.
    if req.quiz_question.is_some()
        && (req.quiz_options.is_none() || req.quiz_correct_answer.is_none())
    {
        return Err(ApiError::BadRequest(
            "quiz_question requires quiz_options and quiz_correct_answer".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, KnowledgePoint>(
        "INSERT INTO knowledge_points
            (recipe_id, step_id, title, content, quiz_question, quiz_options, quiz_correct_answer)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, recipe_id, step_id, title, content, quiz_question,
                   quiz_options, quiz_correct_answer",
    )
    .bind(req.recipe_id)
    .bind(req.step_id)
    .bind(&req.title)
    .bind(&req.content)
    .bind(&req.quiz_question)
    .bind(&req.quiz_options)
    .bind(&req.quiz_correct_answer)
    .fetch_one(&mut *tx)
    .await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::KP_CREATE,
        "knowledge_points",
        Some(row.id),
        json!({
            "recipe_id": row.recipe_id,
            "step_id": row.step_id,
            "title": row.title,
            "quiz": row.quiz_question.is_some(),
        }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "kp_create", "actor={} kp={}", user.user_id(), row.id);
    Ok(HttpResponse::Created().json(row))
}

#[put("/{id}")]
pub async fn update_knowledge_point(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateKnowledgePoint>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    let req = body.into_inner();
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, KnowledgePoint>(
        "UPDATE knowledge_points SET
            title               = COALESCE($1, title),
            content             = COALESCE($2, content),
            quiz_question       = COALESCE($3, quiz_question),
            quiz_options        = COALESCE($4, quiz_options),
            quiz_correct_answer = COALESCE($5, quiz_correct_answer)
         WHERE id = $6
         RETURNING id, recipe_id, step_id, title, content, quiz_question,
                   quiz_options, quiz_correct_answer",
    )
    .bind(&req.title)
    .bind(&req.content)
    .bind(&req.quiz_question)
    .bind(&req.quiz_options)
    .bind(&req.quiz_correct_answer)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let row = row.ok_or_else(|| ApiError::NotFound("knowledge point not found".into()))?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::KP_UPDATE,
        "knowledge_points",
        Some(row.id),
        json!({
            "title_changed": req.title.is_some(),
            "content_changed": req.content.is_some(),
            "quiz_changed": req.quiz_question.is_some()
                || req.quiz_options.is_some()
                || req.quiz_correct_answer.is_some(),
        }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "kp_update", "actor={} kp={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(row))
}

#[delete("/{id}")]
pub async fn delete_knowledge_point(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    let mut tx = pool.begin().await?;
    let affected = sqlx::query("DELETE FROM knowledge_points WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("knowledge point not found".into()));
    }
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::KP_DELETE,
        "knowledge_points",
        Some(id),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "kp_delete", "actor={} kp={}", user.user_id(), id);
    Ok(HttpResponse::NoContent().finish())
}

// -----------------------------------------------------------------------------
// Learning record routes
// -----------------------------------------------------------------------------

#[post("")]
pub async fn record_learning(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    body: web::Json<RecordLearningBody>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();

    // Fetch the KP — 404 if missing, so we never write orphaned records.
    let kp: Option<KnowledgePoint> = sqlx::query_as::<_, KnowledgePoint>(
        "SELECT id, recipe_id, step_id, title, content, quiz_question,
                quiz_options, quiz_correct_answer
         FROM knowledge_points WHERE id = $1",
    )
    .bind(req.knowledge_point_id)
    .fetch_optional(pool.get_ref())
    .await?;
    let kp = kp.ok_or_else(|| ApiError::NotFound("knowledge point not found".into()))?;

    let quiz_score: Option<f64> = match (&kp.quiz_correct_answer, &req.quiz_answer) {
        (Some(correct), Some(given)) => {
            if correct.eq_ignore_ascii_case(given.trim()) {
                Some(1.0)
            } else {
                Some(0.0)
            }
        }
        _ => None,
    };

    let mut tx = pool.begin().await?;

    // Review path: bump review_count on the most recent record for this user+KP,
    // but keep original score. Insert only if no prior record exists.
    if req.review {
        let existing: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM learning_records
             WHERE user_id = $1 AND knowledge_point_id = $2
             ORDER BY completed_at DESC NULLS LAST LIMIT 1",
        )
        .bind(user.user_id())
        .bind(kp.id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some((id,)) = existing {
            let row = sqlx::query_as::<_, LearningRecord>(
                "UPDATE learning_records
                 SET review_count = review_count + 1
                 WHERE id = $1
                 RETURNING id, user_id, knowledge_point_id, work_order_id,
                           quiz_score, time_spent_seconds, review_count, completed_at",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            processing_log::record_tx(
                &mut tx,
                Some(user.user_id()),
                processing_log::actions::LEARNING_RECORD_REVIEW,
                "learning_records",
                Some(row.id),
                json!({
                    "knowledge_point_id": kp.id,
                    "review_count": row.review_count,
                }),
            )
            .await?;
            tx.commit().await?;
            log_info!(MODULE, "record_review", "user={} kp={}", user.user_id(), kp.id);
            return Ok(HttpResponse::Ok().json(row));
        }
    }

    let row = sqlx::query_as::<_, LearningRecord>(
        "INSERT INTO learning_records
            (user_id, knowledge_point_id, work_order_id,
             quiz_score, time_spent_seconds, review_count, completed_at)
         VALUES ($1, $2, $3, $4, $5, 0, NOW())
         RETURNING id, user_id, knowledge_point_id, work_order_id,
                   quiz_score, time_spent_seconds, review_count, completed_at",
    )
    .bind(user.user_id())
    .bind(kp.id)
    .bind(req.work_order_id)
    .bind(quiz_score)
    .bind(req.time_spent_seconds)
    .fetch_one(&mut *tx)
    .await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::LEARNING_RECORD,
        "learning_records",
        Some(row.id),
        json!({
            "knowledge_point_id": kp.id,
            "work_order_id": req.work_order_id,
            "quiz_score": quiz_score,
        }),
    )
    .await?;
    tx.commit().await?;

    // Post-commit, best-effort REVIEW_RESULT notification to the learner when
    // their submission was actually graded (i.e. the KP carried a quiz). The
    // payload carries pass/fail so the UI can render it inline without a
    // follow-up fetch. A notification failure never rolls back the row.
    if let Some(score) = quiz_score {
        let payload = serde_json::json!({
            "knowledge_point_id": kp.id,
            "title": kp.title,
            "quiz_score": score,
            "passed": score >= 1.0,
            "work_order_id": req.work_order_id,
        });
        if let Err(e) = crate::notifications::stub::send(
            pool.get_ref(),
            cfg.get_ref(),
            user.user_id(),
            crate::enums::NotificationTemplate::ReviewResult,
            payload,
        )
        .await
        {
            log_info!(MODULE, "review_notify_failed", "user={} err={}", user.user_id(), e);
        }
    }

    log_info!(
        MODULE,
        "record_create",
        "user={} kp={} score={:?}",
        user.user_id(),
        kp.id,
        quiz_score
    );
    Ok(HttpResponse::Created().json(row))
}

#[get("")]
pub async fn list_records(
    user: AuthedUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    // TECH: own only. SUPER: own branch. ADMIN: all.
    let rows = match user.role() {
        Role::Tech => {
            sqlx::query_as::<_, LearningRecord>(
                "SELECT lr.id, lr.user_id, lr.knowledge_point_id, lr.work_order_id,
                        lr.quiz_score, lr.time_spent_seconds, lr.review_count, lr.completed_at
                 FROM learning_records lr
                 WHERE lr.user_id = $1
                 ORDER BY lr.completed_at DESC NULLS LAST",
            )
            .bind(user.user_id())
            .fetch_all(pool.get_ref())
            .await?
        }
        Role::Super => {
            let branch = require_branch_scope(&user)?;
            sqlx::query_as::<_, LearningRecord>(
                "SELECT lr.id, lr.user_id, lr.knowledge_point_id, lr.work_order_id,
                        lr.quiz_score, lr.time_spent_seconds, lr.review_count, lr.completed_at
                 FROM learning_records lr
                 JOIN users u ON u.id = lr.user_id
                 WHERE u.branch_id = $1
                 ORDER BY lr.completed_at DESC NULLS LAST",
            )
            .bind(branch)
            .fetch_all(pool.get_ref())
            .await?
        }
        Role::Admin => {
            sqlx::query_as::<_, LearningRecord>(
                "SELECT id, user_id, knowledge_point_id, work_order_id,
                        quiz_score, time_spent_seconds, review_count, completed_at
                 FROM learning_records
                 ORDER BY completed_at DESC NULLS LAST",
            )
            .fetch_all(pool.get_ref())
            .await?
        }
    };
    log_info!(MODULE, "records_list", "user={} count={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

/// SUPER/ADMIN can inspect a single record for review workflows.
#[get("/{id}")]
pub async fn get_record(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row: Option<LearningRecord> = sqlx::query_as::<_, LearningRecord>(
        "SELECT id, user_id, knowledge_point_id, work_order_id,
                quiz_score, time_spent_seconds, review_count, completed_at
         FROM learning_records WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool.get_ref())
    .await?;
    let row = row.ok_or_else(|| ApiError::NotFound("record not found".into()))?;

    // Scope: TECH may only see own. SUPER may see branch. ADMIN sees all.
    match user.role() {
        Role::Tech => {
            if row.user_id != user.user_id() {
                return Err(ApiError::NotFound("record not found".into()));
            }
        }
        Role::Super => {
            let u_branch = require_branch_scope(&user)?;
            let owner_branch: Option<Uuid> =
                sqlx::query_scalar("SELECT branch_id FROM users WHERE id = $1")
                    .bind(row.user_id)
                    .fetch_optional(pool.get_ref())
                    .await?
                    .flatten();
            // Fail-closed: SUPER may only see records owned by users pinned
            // to the same branch. A null owner_branch is NOT implicitly
            // shared — this replaces the old "match any None" behavior.
            if owner_branch != Some(u_branch) {
                return Err(ApiError::NotFound("record not found".into()));
            }
            require_any_role(&user, &[Role::Super, Role::Admin])?;
        }
        Role::Admin => {}
    }
    Ok(HttpResponse::Ok().json(row))
}

// -----------------------------------------------------------------------------
// Scopes
// -----------------------------------------------------------------------------

pub fn knowledge_scope() -> actix_web::Scope {
    web::scope("/api/knowledge-points")
        .service(list_knowledge_by_step)
        .service(list_knowledge_points)
        .service(create_knowledge_point)
        .service(update_knowledge_point)
        .service(delete_knowledge_point)
}

pub fn records_scope() -> actix_web::Scope {
    web::scope("/api/learning-records")
        .service(list_records)
        .service(record_learning)
        .service(get_record)
}
