//! Generic per-record version history (PRD §7).
//!
//! Every mutable entity that wants historical retention snapshots a copy
//! of the previous row here via [`snapshot_tx`] before overwriting. The
//! function participates in the caller's transaction so the snapshot
//! and the business write land atomically.
//!
//! Retention is capped to `AppConfig::business::max_versions_per_record`
//! per `(entity_table, entity_id)`. The cap is enforced after each
//! insert by [`enforce_cap_tx`]; older snapshots drop off the bottom of
//! the list, newer ones stay. Pruning uses a session-scoped GUC
//! (`app.record_versions_pruning`) so the DB-level immutability trigger
//! can distinguish legitimate pruning from tampering.
//!
//! This module replaces the step-progress-specific retention loop that
//! used to live inline in `work_orders::progress::upsert_progress`.
//! Call sites (work orders, step progress, future entities) share the
//! same helper so the cap is uniform and the history shape is
//! introspectable via a single table.

use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::log_info;

const MODULE: &str = "versions";

/// Stable identifiers for entity tables that use `record_versions`.
/// Keep this list small and explicit — free-form strings are accepted
/// by the DB, but using these constants keeps call sites greppable.
pub mod entities {
    pub const WORK_ORDERS: &str = "work_orders";
    pub const JOB_STEP_PROGRESS: &str = "job_step_progress";
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecordVersionRow {
    pub id: Uuid,
    pub entity_table: String,
    pub entity_id: Uuid,
    pub version: i32,
    pub snapshot: Value,
    pub actor_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Snapshot a single `(entity_table, entity_id, version)` tuple and
/// enforce the retention cap in the same transaction.
pub async fn snapshot_tx(
    tx: &mut Transaction<'_, Postgres>,
    entity_table: &str,
    entity_id: Uuid,
    version: i32,
    snapshot: Value,
    actor_id: Option<Uuid>,
    max_versions: u32,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO record_versions
            (entity_table, entity_id, version, snapshot, actor_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (entity_table, entity_id, version) DO NOTHING",
    )
    .bind(entity_table)
    .bind(entity_id)
    .bind(version)
    .bind(&snapshot)
    .bind(actor_id)
    .execute(&mut **tx)
    .await?;
    enforce_cap_tx(tx, entity_table, entity_id, max_versions).await?;
    Ok(())
}

/// Keep at most `max_versions` snapshots for `(entity_table, entity_id)`,
/// dropping the oldest by `version` first. Relies on a session-scoped
/// GUC so the immutability trigger lets the prune through without
/// opening an audit back door.
pub async fn enforce_cap_tx(
    tx: &mut Transaction<'_, Postgres>,
    entity_table: &str,
    entity_id: Uuid,
    max_versions: u32,
) -> Result<(), ApiError> {
    // SET LOCAL is tx-scoped — it reverts at commit/rollback, so the
    // "pruning" flag can never leak out of this function.
    sqlx::query("SET LOCAL app.record_versions_pruning = 'on'")
        .execute(&mut **tx)
        .await?;
    let cap = max_versions as i64;
    let deleted = sqlx::query(
        "DELETE FROM record_versions
         WHERE entity_table = $1
           AND entity_id = $2
           AND id IN (
             SELECT id FROM record_versions
             WHERE entity_table = $1 AND entity_id = $2
             ORDER BY version DESC
             OFFSET $3
           )",
    )
    .bind(entity_table)
    .bind(entity_id)
    .bind(cap)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    sqlx::query("SET LOCAL app.record_versions_pruning = 'off'")
        .execute(&mut **tx)
        .await?;
    if deleted > 0 {
        log_info!(
            MODULE,
            "cap_pruned",
            "entity={}/{} pruned={} cap={}",
            entity_table,
            entity_id,
            deleted,
            cap
        );
    }
    Ok(())
}

/// Read the most recent `limit` snapshots for a single entity. Intended
/// for admin review surfaces; falls back to pool access rather than a
/// caller transaction so the read path is simple.
pub async fn list(
    pool: &PgPool,
    entity_table: &str,
    entity_id: Uuid,
    limit: i64,
) -> Result<Vec<RecordVersionRow>, ApiError> {
    let rows = sqlx::query_as::<_, RecordVersionRow>(
        "SELECT id, entity_table, entity_id, version, snapshot, actor_id, created_at
         FROM record_versions
         WHERE entity_table = $1 AND entity_id = $2
         ORDER BY version DESC
         LIMIT $3",
    )
    .bind(entity_table)
    .bind(entity_id)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
