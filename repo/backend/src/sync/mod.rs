//! Offline-first sync engine (PRD §8).
//!
//! Two layers:
//!   1. `trigger` — scheduled change-tracking job. Recomputes ETags for the
//!      tracked entities (work orders, step progress, recipes, tip cards) and
//!      appends a `sync_log` row when an entity's etag has changed. Soft-
//!      deleted entities are tracked as DELETE operations so offline replicas
//!      can converge.
//!   2. `merge` (see `merge.rs`) — the deterministic merge policy that
//!      applies an incoming payload from an offline replica. Invariants
//!      documented in that module.
//!
//! Endpoints that exercise the protocol:
//!   - `GET  /api/sync/changes?since=<rfc3339>` — pull changes since cursor
//!   - `POST /api/sync/step-progress` — replica pushes a single progress row
//!   - `POST /api/sync/work-orders/{id}/delete` — replica propagates soft delete
//!   - `POST /api/admin/sync/conflicts/{id}/resolve` — SUPER/ADMIN signs off

pub mod merge;
pub mod routes;

pub use merge::{merge_step_progress, resolve_conflict, IncomingProgress, MergeOutcome};

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::enums::SyncOperation;
use crate::errors::ApiError;
use crate::etag;
use crate::{log_info, log_warn};

const MODULE: &str = "sync";

pub struct SyncReport {
    pub started_at: chrono::DateTime<Utc>,
    pub finished_at: chrono::DateTime<Utc>,
    pub work_orders_scanned: i64,
    pub work_orders_updated: i64,
    pub work_orders_deleted: i64,
    pub progress_scanned: i64,
    pub progress_updated: i64,
    pub recipes_scanned: i64,
    pub recipes_updated: i64,
    pub tip_cards_scanned: i64,
    pub tip_cards_updated: i64,
    pub conflicts_flagged: i64,
}

impl SyncReport {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "work_orders_scanned": self.work_orders_scanned,
            "work_orders_updated": self.work_orders_updated,
            "work_orders_deleted": self.work_orders_deleted,
            "progress_scanned": self.progress_scanned,
            "progress_updated": self.progress_updated,
            "recipes_scanned": self.recipes_scanned,
            "recipes_updated": self.recipes_updated,
            "tip_cards_scanned": self.tip_cards_scanned,
            "tip_cards_updated": self.tip_cards_updated,
            "conflicts_flagged": self.conflicts_flagged,
        })
    }
}

pub async fn trigger(pool: &PgPool) -> Result<SyncReport, ApiError> {
    let started_at = Utc::now();
    log_info!(MODULE, "trigger", "sync run started");

    let (wo_scanned, wo_updated, wo_deleted) = scan_work_orders(pool).await?;
    let (p_scanned, p_updated) = scan_progress(pool).await?;
    let (r_scanned, r_updated) = scan_recipes(pool).await?;
    let (t_scanned, t_updated) = scan_tip_cards(pool).await?;
    let conflicts = count_unresolved_conflicts(pool).await?;

    let report = SyncReport {
        started_at,
        finished_at: Utc::now(),
        work_orders_scanned: wo_scanned,
        work_orders_updated: wo_updated,
        work_orders_deleted: wo_deleted,
        progress_scanned: p_scanned,
        progress_updated: p_updated,
        recipes_scanned: r_scanned,
        recipes_updated: r_updated,
        tip_cards_scanned: t_scanned,
        tip_cards_updated: t_updated,
        conflicts_flagged: conflicts,
    };
    log_info!(
        MODULE,
        "trigger",
        "done wo_scan={} wo_upd={} wo_del={} prog_scan={} prog_upd={} r_upd={} t_upd={} conflicts={}",
        report.work_orders_scanned,
        report.work_orders_updated,
        report.work_orders_deleted,
        report.progress_scanned,
        report.progress_updated,
        report.recipes_updated,
        report.tip_cards_updated,
        report.conflicts_flagged
    );
    Ok(report)
}

/// Scan work_orders including soft-deleted rows so replicas see DELETE events.
async fn scan_work_orders(pool: &PgPool) -> Result<(i64, i64, i64), ApiError> {
    let rows: Vec<(
        Uuid,
        Option<String>,
        String,
        i32,
        chrono::DateTime<Utc>,
        Option<chrono::DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT id, etag, state::text, version_count, updated_at, deleted_at
         FROM work_orders",
    )
    .fetch_all(pool)
    .await?;
    let scanned = rows.len() as i64;
    let mut updated = 0i64;
    let mut deleted = 0i64;
    for (id, old_etag, state, version_count, updated_at, deleted_at) in rows {
        let is_deleted = deleted_at.is_some();
        // Etag incorporates deleted_at so a soft delete flips the etag.
        let new_etag = etag::from_parts([
            id.to_string(),
            state.clone(),
            version_count.to_string(),
            updated_at.timestamp().to_string(),
            deleted_at.map(|d| d.timestamp().to_string()).unwrap_or_default(),
        ]);
        if old_etag.as_deref() != Some(new_etag.as_str()) {
            let op = if is_deleted { "DELETE" } else { "UPDATE" };
            sqlx::query(
                "INSERT INTO sync_log (entity_table, entity_id, operation, old_etag, new_etag)
                 VALUES ('work_orders', $1, $2::sync_operation, $3, $4)",
            )
            .bind(id)
            .bind(op)
            .bind(&old_etag)
            .bind(&new_etag)
            .execute(pool)
            .await?;
            sqlx::query("UPDATE work_orders SET etag = $1 WHERE id = $2")
                .bind(&new_etag)
                .bind(id)
                .execute(pool)
                .await?;
            if is_deleted {
                deleted += 1;
            } else {
                updated += 1;
            }
        }
    }
    Ok((scanned, updated, deleted))
}

async fn scan_progress(pool: &PgPool) -> Result<(i64, i64), ApiError> {
    let rows: Vec<(Uuid, Option<String>, String, i32)> = sqlx::query_as(
        "SELECT id, etag, status::text, version FROM job_step_progress",
    )
    .fetch_all(pool)
    .await?;
    let scanned = rows.len() as i64;
    let mut updated = 0i64;
    for (id, old_etag, status, version) in rows {
        let new_etag = etag::from_parts([id.to_string(), status, version.to_string()]);
        if old_etag.as_deref() != Some(new_etag.as_str()) {
            sqlx::query(
                "INSERT INTO sync_log (entity_table, entity_id, operation, old_etag, new_etag)
                 VALUES ('job_step_progress', $1, $2, $3, $4)",
            )
            .bind(id)
            .bind(SyncOperation::Update)
            .bind(&old_etag)
            .bind(&new_etag)
            .execute(pool)
            .await?;
            sqlx::query("UPDATE job_step_progress SET etag = $1 WHERE id = $2")
                .bind(&new_etag)
                .bind(id)
                .execute(pool)
                .await?;
            updated += 1;
        }
    }
    Ok((scanned, updated))
}

/// Scan recipes for change-tracking. Recipes have no etag column, so we hash a
/// stable snapshot of the row into sync_log only when the computed etag
/// differs from the most recent sync_log entry for the same row.
async fn scan_recipes(pool: &PgPool) -> Result<(i64, i64), ApiError> {
    let rows: Vec<(Uuid, String, Option<String>, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, name, description, updated_at FROM recipes",
    )
    .fetch_all(pool)
    .await?;
    let scanned = rows.len() as i64;
    let mut updated = 0i64;
    for (id, name, description, updated_at) in rows {
        let new_etag = etag::from_parts([
            id.to_string(),
            name,
            description.unwrap_or_default(),
            updated_at.timestamp().to_string(),
        ]);
        let last: Option<Option<String>> = sqlx::query_scalar(
            "SELECT new_etag FROM sync_log
             WHERE entity_table = 'recipes' AND entity_id = $1
             ORDER BY synced_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        let last_etag = last.flatten();
        if last_etag.as_deref() != Some(new_etag.as_str()) {
            sqlx::query(
                "INSERT INTO sync_log (entity_table, entity_id, operation, old_etag, new_etag)
                 VALUES ('recipes', $1, 'UPDATE', $2, $3)",
            )
            .bind(id)
            .bind(&last_etag)
            .bind(&new_etag)
            .execute(pool)
            .await?;
            updated += 1;
        }
    }
    Ok((scanned, updated))
}

async fn scan_tip_cards(pool: &PgPool) -> Result<(i64, i64), ApiError> {
    let rows: Vec<(Uuid, String, String, bool, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, title, content, is_pinned, updated_at FROM tip_cards",
    )
    .fetch_all(pool)
    .await?;
    let scanned = rows.len() as i64;
    let mut updated = 0i64;
    for (id, title, content, pinned, updated_at) in rows {
        let new_etag = etag::from_parts([
            id.to_string(),
            title,
            content,
            pinned.to_string(),
            updated_at.timestamp().to_string(),
        ]);
        let last: Option<Option<String>> = sqlx::query_scalar(
            "SELECT new_etag FROM sync_log
             WHERE entity_table = 'tip_cards' AND entity_id = $1
             ORDER BY synced_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        let last_etag = last.flatten();
        if last_etag.as_deref() != Some(new_etag.as_str()) {
            sqlx::query(
                "INSERT INTO sync_log (entity_table, entity_id, operation, old_etag, new_etag)
                 VALUES ('tip_cards', $1, 'UPDATE', $2, $3)",
            )
            .bind(id)
            .bind(&last_etag)
            .bind(&new_etag)
            .execute(pool)
            .await?;
            updated += 1;
        }
    }
    Ok((scanned, updated))
}

async fn count_unresolved_conflicts(pool: &PgPool) -> Result<i64, ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log
         WHERE conflict_flagged = TRUE AND conflict_resolved_by IS NULL",
    )
    .fetch_one(pool)
    .await?;
    if n > 0 {
        log_warn!(MODULE, "conflicts", "{} unresolved conflicts pending SUPER review", n);
    }
    Ok(n)
}

/// Record a DELETE event for a soft-deleted work order. Called from
/// `DELETE /api/work-orders/{id}` so a replica can converge immediately
/// instead of waiting for the next scan tick.
pub async fn log_soft_delete(
    pool: &PgPool,
    entity_table: &str,
    entity_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation, old_etag, new_etag)
         VALUES ($1, $2, 'DELETE', NULL, NULL)",
    )
    .bind(entity_table)
    .bind(entity_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Transactional variant — participates in the caller's transaction so the
/// soft-delete row and the DELETE tombstone land atomically with the audit
/// log and any other state-changing writes in the same request.
pub async fn log_soft_delete_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    entity_table: &str,
    entity_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO sync_log (entity_table, entity_id, operation, old_etag, new_etag)
         VALUES ($1, $2, 'DELETE', NULL, NULL)",
    )
    .bind(entity_table)
    .bind(entity_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
