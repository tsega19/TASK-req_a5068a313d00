//! Step progress: list, upsert, and version history pruning.

use actix_web::{get, put, web, HttpResponse};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::config::AppConfig;
use crate::enums::StepProgressStatus;
use crate::errors::ApiError;
use crate::etag;
use crate::middleware::rbac::AuthedUser;
use crate::processing_log;
use crate::work_orders::routes::load_visible;
use crate::log_info;

const MODULE: &str = "work_orders";

#[derive(Debug, Deserialize)]
pub struct ProgressUpsert {
    pub status: StepProgressStatus,
    pub notes: Option<String>,
    pub timer_state: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ProgressRow {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub step_id: Uuid,
    pub status: StepProgressStatus,
    pub started_at: Option<chrono::DateTime<Utc>>,
    pub paused_at: Option<chrono::DateTime<Utc>>,
    pub completed_at: Option<chrono::DateTime<Utc>>,
    pub notes: Option<String>,
    pub timer_state_snapshot: Option<serde_json::Value>,
    pub etag: Option<String>,
    pub version: i32,
}

#[get("/{id}/progress")]
pub async fn list_progress(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let _wo = load_visible(&pool, &user, id).await?;
    let rows = sqlx::query_as::<_, ProgressRow>(
        "SELECT id, work_order_id, step_id, status, started_at, paused_at,
                completed_at, notes, timer_state_snapshot, etag, version
         FROM job_step_progress
         WHERE work_order_id = $1
         ORDER BY (SELECT step_order FROM recipe_steps WHERE id = job_step_progress.step_id) ASC",
    )
    .bind(id)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "progress_list", "user={} wo={} rows={}", user.user_id(), id, rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

#[put("/{id}/steps/{step_id}/progress")]
pub async fn upsert_progress(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    path: web::Path<(Uuid, Uuid)>,
    body: web::Json<ProgressUpsert>,
) -> Result<HttpResponse, ApiError> {
    let (wo_id, step_id) = path.into_inner();
    let wo = load_visible(&pool, &user, wo_id).await?;

    // Object-level: TECH must own the work order to mutate step progress.
    if matches!(user.role(), Role::Tech) && wo.assigned_tech_id != Some(user.user_id()) {
        return Err(ApiError::Forbidden("not assigned to this work order".into()));
    }
    // SUPER/ADMIN can observe/fix progress within scope; TECH is the primary actor.

    let req = body.into_inner();
    let now = Utc::now();

    // Fetch existing progress (if any) for version snapshot.
    let existing: Option<ProgressRow> = sqlx::query_as::<_, ProgressRow>(
        "SELECT id, work_order_id, step_id, status, started_at, paused_at,
                completed_at, notes, timer_state_snapshot, etag, version
         FROM job_step_progress WHERE work_order_id = $1 AND step_id = $2",
    )
    .bind(wo_id)
    .bind(step_id)
    .fetch_optional(pool.get_ref())
    .await?;

    let (started_at, paused_at, completed_at) = match req.status {
        StepProgressStatus::Pending => (None, None, None),
        StepProgressStatus::InProgress => (
            existing.as_ref().and_then(|e| e.started_at).or(Some(now)),
            None,
            None,
        ),
        StepProgressStatus::Paused => (
            existing.as_ref().and_then(|e| e.started_at),
            Some(now),
            None,
        ),
        StepProgressStatus::Completed => (
            existing.as_ref().and_then(|e| e.started_at).or(Some(now)),
            None,
            Some(now),
        ),
    };

    let etag_v = etag::from_parts([
        wo_id.to_string(),
        step_id.to_string(),
        format!("{:?}", req.status),
        now.timestamp().to_string(),
    ]);

    let mut tx = pool.begin().await?;

    let row: ProgressRow = match existing {
        Some(prev) => {
            // Snapshot previous version before overwriting.
            let snapshot = serde_json::to_value(&prev).unwrap_or(json!({}));
            sqlx::query(
                "INSERT INTO job_step_progress_versions (progress_id, snapshot, version)
                 VALUES ($1, $2, $3)",
            )
            .bind(prev.id)
            .bind(&snapshot)
            .bind(prev.version)
            .execute(&mut *tx)
            .await?;

            let next_version = prev.version + 1;
            let updated = sqlx::query_as::<_, ProgressRow>(
                "UPDATE job_step_progress
                 SET status = $1, notes = COALESCE($2, notes),
                     timer_state_snapshot = COALESCE($3, timer_state_snapshot),
                     started_at = $4, paused_at = $5, completed_at = $6,
                     etag = $7, version = $8, updated_at = NOW()
                 WHERE id = $9
                 RETURNING id, work_order_id, step_id, status, started_at, paused_at,
                           completed_at, notes, timer_state_snapshot, etag, version",
            )
            .bind(req.status)
            .bind(&req.notes)
            .bind(&req.timer_state)
            .bind(started_at)
            .bind(paused_at)
            .bind(completed_at)
            .bind(&etag_v)
            .bind(next_version)
            .bind(prev.id)
            .fetch_one(&mut *tx)
            .await?;

            // Enforce version cap (PRD §7, max 30).
            let cap = cfg.business.max_versions_per_progress as i64;
            sqlx::query(
                "DELETE FROM job_step_progress_versions
                 WHERE progress_id = $1
                   AND id IN (
                     SELECT id FROM job_step_progress_versions
                     WHERE progress_id = $1
                     ORDER BY version DESC
                     OFFSET $2
                   )",
            )
            .bind(updated.id)
            .bind(cap)
            .execute(&mut *tx)
            .await?;

            updated
        }
        None => {
            sqlx::query_as::<_, ProgressRow>(
                "INSERT INTO job_step_progress
                    (work_order_id, step_id, status, notes, timer_state_snapshot,
                     started_at, paused_at, completed_at, etag, version)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,1)
                 RETURNING id, work_order_id, step_id, status, started_at, paused_at,
                           completed_at, notes, timer_state_snapshot, etag, version",
            )
            .bind(wo_id)
            .bind(step_id)
            .bind(req.status)
            .bind(&req.notes)
            .bind(&req.timer_state)
            .bind(started_at)
            .bind(paused_at)
            .bind(completed_at)
            .bind(&etag_v)
            .fetch_one(&mut *tx)
            .await?
        }
    };

    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::STEP_PROGRESS_UPSERT,
        "job_step_progress",
        Some(row.id),
        json!({
            "work_order_id": wo_id,
            "step_id": step_id,
            "status": format!("{:?}", req.status),
            "version": row.version,
            "timer_state_present": req.timer_state.is_some(),
        }),
    )
    .await?;
    tx.commit().await?;
    log_info!(
        MODULE,
        "progress_upsert",
        "user={} wo={} step={} status={:?} v={}",
        user.user_id(),
        wo_id,
        step_id,
        req.status,
        row.version
    );
    Ok(HttpResponse::Ok().json(row))
}
