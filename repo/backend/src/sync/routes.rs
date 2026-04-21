//! HTTP surface for the sync engine.
//!
//! Pull side (replica → server):
//!   - `GET  /api/sync/changes?since=<rfc3339>` — list change events after the
//!     given cursor; covers work orders, progress, recipes, tip cards.
//!
//! Push side (replica → server):
//!   - `POST /api/sync/step-progress` — replica pushes a single progress row
//!     (deterministic merge in `merge::merge_step_progress`).
//!   - `POST /api/sync/work-orders/{id}/delete` — replica propagates a soft
//!     delete issued while offline (ADMIN only).
//!
//! Supervisor surface:
//!   - `GET  /api/sync/conflicts` — list unresolved merge conflicts.
//!   - `POST /api/sync/conflicts/{id}/resolve` — acknowledge a conflict.

use actix_web::{get, post, web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::errors::ApiError;
use crate::log_info;
use crate::middleware::rbac::{require_any_role, require_branch, require_role, AuthedUser};
use crate::processing_log;
use crate::sync::merge::{merge_step_progress, resolve_conflict, IncomingProgress, MergeOutcome};
use crate::work_orders::routes::load_visible;

const MODULE: &str = "sync";

#[derive(Debug, Serialize)]
pub struct MergeResponse {
    pub outcome: String,
    pub conflict: bool,
}

impl From<MergeOutcome> for MergeResponse {
    fn from(o: MergeOutcome) -> Self {
        let conflict = matches!(o, MergeOutcome::Conflict);
        let outcome = match o {
            MergeOutcome::Applied => "applied",
            MergeOutcome::RejectedCompleted => "rejected_completed",
            MergeOutcome::RejectedOlder => "rejected_older",
            MergeOutcome::Conflict => "conflict",
        };
        MergeResponse { outcome: outcome.into(), conflict }
    }
}

#[post("/step-progress")]
pub async fn post_step_progress(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    body: web::Json<IncomingProgress>,
) -> Result<HttpResponse, ApiError> {
    let incoming = body.into_inner();
    // Object-level: caller must be able to see the work order this row belongs
    // to. `load_visible` already returns 404 on scope miss, so we don't leak.
    let wo = load_visible(&pool, &user, incoming.work_order_id).await?;
    if matches!(user.role(), Role::Tech) && wo.assigned_tech_id != Some(user.user_id()) {
        return Err(ApiError::Forbidden("not assigned to this work order".into()));
    }
    let outcome = merge_step_progress(pool.get_ref(), &incoming).await?;
    // Immutable audit (PRD §7 / audit AR-1 Medium): record the push outcome
    // so operator replays are reconstructible from the processing_log alone.
    let mut tx = pool.begin().await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::SYNC_PROGRESS_PUSH,
        "job_step_progress",
        None,
        serde_json::json!({
            "work_order_id": incoming.work_order_id,
            "step_id": incoming.step_id,
            "outcome": format!("{:?}", outcome),
        }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "push_progress", "user={} wo={} step={} outcome={:?}",
        user.user_id(), incoming.work_order_id, incoming.step_id, outcome);
    Ok(HttpResponse::Ok().json(MergeResponse::from(outcome)))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConflictRow {
    pub id: Uuid,
    pub entity_table: String,
    pub entity_id: Uuid,
    pub new_etag: Option<String>,
    pub synced_at: chrono::DateTime<chrono::Utc>,
}

#[get("/conflicts")]
pub async fn list_conflicts(
    user: AuthedUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    require_any_role(&user, &[Role::Super, Role::Admin])?;
    // Audit-2 Medium #5: SUPER previously saw every unresolved conflict
    // regardless of branch. Scope work-order and step-progress conflicts to
    // the caller's branch; ADMIN sees the full feed. Global entities
    // (recipes / tip_cards) stay visible to both because they are not
    // branch-scoped in the schema.
    let rows = match user.role() {
        Role::Admin => sqlx::query_as::<_, ConflictRow>(
            "SELECT id, entity_table, entity_id, new_etag, synced_at
             FROM sync_log
             WHERE conflict_flagged = TRUE AND conflict_resolved_by IS NULL
             ORDER BY synced_at ASC",
        )
        .fetch_all(pool.get_ref())
        .await?,
        Role::Super => {
            let branch = require_branch(&user)?;
            sqlx::query_as::<_, ConflictRow>(
                "SELECT s.id, s.entity_table, s.entity_id, s.new_etag, s.synced_at
                 FROM sync_log s
                 WHERE s.conflict_flagged = TRUE
                   AND s.conflict_resolved_by IS NULL
                   AND (
                     s.entity_table IN ('recipes', 'tip_cards')
                     OR (
                       s.entity_table = 'work_orders' AND EXISTS (
                         SELECT 1 FROM work_orders w
                         WHERE w.id = s.entity_id AND w.branch_id = $1
                       )
                     )
                     OR (
                       s.entity_table = 'job_step_progress' AND EXISTS (
                         SELECT 1 FROM job_step_progress p
                         JOIN work_orders w ON w.id = p.work_order_id
                         WHERE p.id = s.entity_id AND w.branch_id = $1
                       )
                     )
                   )
                 ORDER BY s.synced_at ASC",
            )
            .bind(branch)
            .fetch_all(pool.get_ref())
            .await?
        }
        Role::Tech => unreachable!("require_any_role above rejects TECH"),
    };
    log_info!(MODULE, "conflicts_list", "user={} role={} count={}", user.user_id(), user.role(), rows.len());
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": rows, "total": rows.len() })))
}

#[derive(Debug, Deserialize)]
pub struct ResolveBody {
    /// Operator acknowledges the merge outcome.
    pub acknowledged: bool,
}

#[post("/conflicts/{id}/resolve")]
pub async fn post_resolve_conflict(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<ResolveBody>,
) -> Result<HttpResponse, ApiError> {
    require_any_role(&user, &[Role::Super, Role::Admin])?;
    let id = path.into_inner();
    if !body.acknowledged {
        return Err(ApiError::BadRequest("acknowledged=true required".into()));
    }
    // Audit-2 Medium #5: a SUPER must only acknowledge conflicts for their
    // own branch. We 404 (not 403) on scope miss to avoid leaking which IDs
    // exist in other branches.
    if matches!(user.role(), Role::Super) {
        let branch = require_branch(&user)?;
        let in_scope: Option<bool> = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM sync_log s
                 WHERE s.id = $1
                   AND (
                     s.entity_table IN ('recipes', 'tip_cards')
                     OR (s.entity_table = 'work_orders' AND EXISTS (
                         SELECT 1 FROM work_orders w
                         WHERE w.id = s.entity_id AND w.branch_id = $2))
                     OR (s.entity_table = 'job_step_progress' AND EXISTS (
                         SELECT 1 FROM job_step_progress p
                         JOIN work_orders w ON w.id = p.work_order_id
                         WHERE p.id = s.entity_id AND w.branch_id = $2))
                   )
             )",
        )
        .bind(id)
        .bind(branch)
        .fetch_optional(pool.get_ref())
        .await?;
        if !in_scope.unwrap_or(false) {
            return Err(ApiError::NotFound("conflict not found".into()));
        }
    }
    resolve_conflict(pool.get_ref(), id, user.user_id()).await?;
    // Audit row for the conflict acknowledgement (PRD §7 / audit AR-1 Medium).
    let mut tx = pool.begin().await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::SYNC_CONFLICT_RESOLVE,
        "sync_log",
        Some(id),
        serde_json::json!({ "acknowledged": true }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "conflict_resolve", "user={} sync_log_id={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(serde_json::json!({ "resolved": true })))
}

// -----------------------------------------------------------------------------
// Pull side: list changes since cursor
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ChangesQuery {
    /// RFC3339 cursor. Omit for "from the beginning".
    pub since: Option<String>,
    /// Optional entity filter (e.g. "work_orders").
    pub entity: Option<String>,
    /// Max rows returned. Clamped to [1, 1000]. Default 500.
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChangeRow {
    pub id: Uuid,
    pub entity_table: String,
    pub entity_id: Uuid,
    pub operation: String,
    pub old_etag: Option<String>,
    pub new_etag: Option<String>,
    pub synced_at: DateTime<Utc>,
    pub conflict_flagged: bool,
}

#[get("/changes")]
pub async fn list_changes(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<ChangesQuery>,
) -> Result<HttpResponse, ApiError> {
    // The changes feed is scoped to what the caller can already see on the
    // read side — otherwise a TECH could enumerate entity_ids belonging to
    // other branches via sync_log metadata alone, even though the follow-up
    // reads would 404. Scoping rules mirror load_visible for work orders:
    //
    //   - ADMIN: full feed.
    //   - SUPER: work_order rows within their branch (plus branch-less),
    //            plus job_step_progress rows attached to those work orders,
    //            plus global recipe/tip-card rows (not branch-scoped).
    //   - TECH:  only rows for entities they are assigned to (work orders
    //            + progress rows); recipes/tip-cards tied to those WOs.
    //
    // This keeps `/api/sync/changes` from leaking cross-scope UUIDs.
    let q = q.into_inner();
    let since = match q.since.as_deref() {
        None => None,
        Some(s) => Some(
            DateTime::parse_from_rfc3339(s)
                .map_err(|e| ApiError::BadRequest(format!("invalid since cursor: {}", e)))?
                .with_timezone(&Utc),
        ),
    };
    let limit = q.limit.unwrap_or(500).clamp(1, 1000);

    let rows: Vec<ChangeRow> = match user.role() {
        Role::Admin => sqlx::query_as::<_, ChangeRow>(
            "SELECT id, entity_table, entity_id, operation::text AS operation,
                    old_etag, new_etag, synced_at, conflict_flagged
             FROM sync_log
             WHERE ($1::timestamptz IS NULL OR synced_at > $1)
               AND ($2::text IS NULL OR entity_table = $2)
             ORDER BY synced_at ASC
             LIMIT $3",
        )
        .bind(since)
        .bind(q.entity.as_deref())
        .bind(limit)
        .fetch_all(pool.get_ref())
        .await?,

        Role::Super => {
            // Fail-closed (audit AR-1 High): a SUPER without a branch claim
            // would have widened through both the `$3 IS NULL` escape and the
            // `w.branch_id IS NULL` leg, leaking cross-branch UUIDs. Require
            // the branch and match strictly.
            let branch = require_branch(&user)?;
            sqlx::query_as::<_, ChangeRow>(
                "SELECT s.id, s.entity_table, s.entity_id, s.operation::text AS operation,
                        s.old_etag, s.new_etag, s.synced_at, s.conflict_flagged
                 FROM sync_log s
                 WHERE ($1::timestamptz IS NULL OR s.synced_at > $1)
                   AND ($2::text IS NULL OR s.entity_table = $2)
                   AND (
                     s.entity_table IN ('recipes', 'tip_cards')
                     OR (
                       s.entity_table = 'work_orders' AND EXISTS (
                         SELECT 1 FROM work_orders w
                         WHERE w.id = s.entity_id AND w.branch_id = $3
                       )
                     )
                     OR (
                       s.entity_table = 'job_step_progress' AND EXISTS (
                         SELECT 1 FROM job_step_progress p
                         JOIN work_orders w ON w.id = p.work_order_id
                         WHERE p.id = s.entity_id AND w.branch_id = $3
                       )
                     )
                   )
                 ORDER BY s.synced_at ASC
                 LIMIT $4",
            )
            .bind(since)
            .bind(q.entity.as_deref())
            .bind(branch)
            .bind(limit)
            .fetch_all(pool.get_ref())
            .await?
        }

        Role::Tech => sqlx::query_as::<_, ChangeRow>(
            "SELECT s.id, s.entity_table, s.entity_id, s.operation::text AS operation,
                    s.old_etag, s.new_etag, s.synced_at, s.conflict_flagged
             FROM sync_log s
             WHERE ($1::timestamptz IS NULL OR s.synced_at > $1)
               AND ($2::text IS NULL OR s.entity_table = $2)
               AND (
                 s.entity_table = 'recipes'
                 OR s.entity_table = 'tip_cards'
                 OR (
                   s.entity_table = 'work_orders' AND EXISTS (
                     SELECT 1 FROM work_orders w
                     WHERE w.id = s.entity_id AND w.assigned_tech_id = $3
                   )
                 )
                 OR (
                   s.entity_table = 'job_step_progress' AND EXISTS (
                     SELECT 1 FROM job_step_progress p
                     JOIN work_orders w ON w.id = p.work_order_id
                     WHERE p.id = s.entity_id AND w.assigned_tech_id = $3
                   )
                 )
               )
             ORDER BY s.synced_at ASC
             LIMIT $4",
        )
        .bind(since)
        .bind(q.entity.as_deref())
        .bind(user.user_id())
        .bind(limit)
        .fetch_all(pool.get_ref())
        .await?,
    };

    // Cursor: max synced_at in returned rows, or echoed input.
    let next_cursor = rows
        .last()
        .map(|r| r.synced_at)
        .or(since)
        .map(|d| d.to_rfc3339());

    log_info!(
        MODULE,
        "changes",
        "user={} role={} since={:?} entity={:?} count={}",
        user.user_id(),
        user.role(),
        q.since,
        q.entity,
        rows.len()
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "data": rows,
        "next_cursor": next_cursor,
        "count": rows.len(),
    })))
}

// -----------------------------------------------------------------------------
// Push side: propagate a soft delete issued offline
// -----------------------------------------------------------------------------

#[post("/work-orders/{id}/delete")]
pub async fn push_work_order_delete(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    // Soft-delete propagation must be ADMIN; matches the live DELETE handler.
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    // Run the soft-delete + sync_log + processing_log writes atomically so a
    // partial failure doesn't leave an untracked propagation (audit AR-1 Medium).
    let mut tx = pool.begin().await?;
    let affected = sqlx::query(
        "UPDATE work_orders SET deleted_at = COALESCE(deleted_at, NOW()), updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("work order not found".into()));
    }
    crate::sync::log_soft_delete_tx(&mut tx, "work_orders", id).await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::SYNC_WO_DELETE_PUSH,
        "work_orders",
        Some(id),
        serde_json::json!({ "propagated": true }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "push_delete", "user={} wo={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true, "entity_table": "work_orders", "entity_id": id })))
}

pub fn scope() -> actix_web::Scope {
    web::scope("/api/sync")
        .service(list_changes)
        .service(post_step_progress)
        .service(list_conflicts)
        .service(post_resolve_conflict)
        .service(push_work_order_delete)
}
