//! Soft-delete retention pruning (PRD §7 — 90-day retention).
//!
//! Walks tables with a `deleted_at` column and hard-deletes rows whose
//! `deleted_at` predates the configured window. `work_order_transitions` is
//! immutable (see migration 0001), and its FK to `work_orders` is ON DELETE
//! CASCADE. To reconcile retention with immutability, migration 0002 teaches
//! the immutability trigger to honour a per-transaction session variable
//! `fieldops.retention_prune`. We set it here, and reset afterwards, so
//! nobody else can silently mutate the audit trail.
//!
//! Callers:
//!   - Daily background worker (`spawn_retention_worker`).
//!   - `POST /api/admin/retention/prune` — ADMIN ad-hoc trigger.

use sqlx::PgPool;

use crate::config::AppConfig;
use crate::errors::ApiError;
use crate::log_info;

const MODULE: &str = "retention";

pub struct PruneReport {
    pub users_pruned: i64,
    pub work_orders_pruned: i64,
    pub retention_days: i64,
}

impl PruneReport {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "users_pruned": self.users_pruned,
            "work_orders_pruned": self.work_orders_pruned,
            "retention_days": self.retention_days,
        })
    }
}

pub async fn prune(pool: &PgPool, cfg: &AppConfig) -> Result<PruneReport, ApiError> {
    let days = cfg.business.soft_delete_retention_days as i64;

    let mut tx = pool.begin().await?;
    // Permit cascaded delete of the immutable transition log for this TX only.
    sqlx::query("SET LOCAL fieldops.retention_prune = 'on'")
        .execute(&mut *tx)
        .await?;

    let wo_pruned: u64 = sqlx::query(
        "DELETE FROM work_orders
         WHERE deleted_at IS NOT NULL
           AND deleted_at < NOW() - make_interval(days => $1::int)",
    )
    .bind(days as i32)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let users_pruned: u64 = sqlx::query(
        "DELETE FROM users
         WHERE deleted_at IS NOT NULL
           AND deleted_at < NOW() - make_interval(days => $1::int)",
    )
    .bind(days as i32)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;

    let report = PruneReport {
        users_pruned: users_pruned as i64,
        work_orders_pruned: wo_pruned as i64,
        retention_days: days,
    };
    log_info!(
        MODULE,
        "prune",
        "users={} work_orders={} retention_days={}",
        report.users_pruned,
        report.work_orders_pruned,
        report.retention_days
    );
    Ok(report)
}
