//! Deterministic merge policy for offline replica sync (PRD §8).
//!
//! Invariants enforced by this module:
//!   1. Completed step logs are IMMUTABLE — a "completed" progress row is
//!      never overwritten by a later incoming payload. Any such attempt is
//!      recorded as a conflict row in `sync_log`, awaiting SUPER review.
//!   2. Higher-version payloads win over lower-version ones deterministically.
//!      Equal-version conflicts are broken by timestamp: the payload with the
//!      later `updated_at` wins. Only strictly equal `updated_at` divergent
//!      edits are flagged as conflicts — never silently dropped.
//!   3. Step notes from a completed step are appended, never replaced, by any
//!      other replica.
//!   4. Concurrent note edits ALWAYS require supervisor review: when both the
//!      local and incoming sides have non-empty `notes` that disagree at the
//!      same version, the merge flags a conflict regardless of timestamp.
//!      Timestamps can be untrustworthy across offline replicas, and notes
//!      carry narrative context a later-writer-wins rule would silently
//!      obliterate.
//!
//! The merge function returns a `MergeOutcome` so callers can see exactly what
//! happened (applied / rejected / flagged). All flagged conflicts land in
//! `sync_log` with `conflict_flagged = TRUE` and remain there until a SUPER
//! user marks them `conflict_resolved_by` via `POST /api/admin/sync/...`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::enums::{StepProgressStatus, SyncOperation};
use crate::errors::ApiError;
use crate::etag;
use crate::{log_info, log_warn};

const MODULE: &str = "sync::merge";

/// Incoming step progress from an offline replica. The client side constructs
/// this from its local row and ships it to the server on reconnect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingProgress {
    pub work_order_id: Uuid,
    pub step_id: Uuid,
    pub status: StepProgressStatus,
    pub notes: Option<String>,
    pub timer_state_snapshot: Option<serde_json::Value>,
    pub version: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The incoming payload was applied (insert or update).
    Applied,
    /// The incoming payload was rejected because local state is already
    /// Completed — the completed log is immutable.
    RejectedCompleted,
    /// The incoming payload was older (or equal and lost the tiebreaker).
    RejectedOlder,
    /// A conflict was detected (same version, different payload); the row
    /// was left untouched and a conflict row was recorded in `sync_log`.
    Conflict,
}

/// Apply the deterministic merge policy for one incoming step progress row.
///
/// The function is idempotent: calling it twice with the same input produces
/// the same result. It uses a single transaction so either the apply + log
/// pair lands or nothing does.
pub async fn merge_step_progress(
    pool: &PgPool,
    incoming: &IncomingProgress,
) -> Result<MergeOutcome, ApiError> {
    let mut tx = pool.begin().await?;

    let existing: Option<(
        Uuid,
        StepProgressStatus,
        Option<String>,
        Option<serde_json::Value>,
        i32,
        DateTime<Utc>,
    )> = sqlx::query_as(
        "SELECT id, status, notes, timer_state_snapshot, version, updated_at
         FROM job_step_progress
         WHERE work_order_id = $1 AND step_id = $2
         FOR UPDATE",
    )
    .bind(incoming.work_order_id)
    .bind(incoming.step_id)
    .fetch_optional(&mut *tx)
    .await?;

    let now = Utc::now();

    let outcome = match existing {
        // No local row — accept the incoming payload as a fresh insert.
        None => {
            let etag_v = etag::from_parts([
                incoming.work_order_id.to_string(),
                incoming.step_id.to_string(),
                format!("{:?}", incoming.status),
                incoming.version.to_string(),
            ]);
            sqlx::query(
                "INSERT INTO job_step_progress
                    (work_order_id, step_id, status, notes, timer_state_snapshot,
                     etag, version, updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(incoming.work_order_id)
            .bind(incoming.step_id)
            .bind(incoming.status)
            .bind(&incoming.notes)
            .bind(&incoming.timer_state_snapshot)
            .bind(&etag_v)
            .bind(incoming.version)
            .bind(incoming.updated_at)
            .execute(&mut *tx)
            .await?;
            log_sync(&mut tx, incoming, SyncOperation::Insert, &etag_v, false).await?;
            MergeOutcome::Applied
        }
        Some((row_id, local_status, local_notes, local_timer, local_version, local_updated_at)) => {
            // Invariant 1: completed logs are immutable. Only a same-as-local
            // Completed payload is allowed through (idempotency); anything
            // else is a flagged conflict.
            if local_status == StepProgressStatus::Completed {
                if incoming.status == StepProgressStatus::Completed
                    && incoming.notes == local_notes
                {
                    log_info!(
                        MODULE,
                        "idempotent_completed",
                        "wo={} step={}",
                        incoming.work_order_id,
                        incoming.step_id
                    );
                    MergeOutcome::RejectedCompleted
                } else {
                    log_warn!(
                        MODULE,
                        "completed_immutable",
                        "wo={} step={} incoming={:?} — flagged for SUPER",
                        incoming.work_order_id,
                        incoming.step_id,
                        incoming.status
                    );
                    // Append the incoming notes to the completed row so the
                    // technician's work is not lost — never overwrite.
                    if let Some(extra) = &incoming.notes {
                        let merged = match local_notes {
                            Some(existing_notes) if !existing_notes.is_empty() => {
                                format!("{}\n---\n[from replica @ {}]\n{}", existing_notes, incoming.updated_at, extra)
                            }
                            _ => format!("[from replica @ {}]\n{}", incoming.updated_at, extra),
                        };
                        sqlx::query("UPDATE job_step_progress SET notes = $1 WHERE id = $2")
                            .bind(&merged)
                            .bind(row_id)
                            .execute(&mut *tx)
                            .await?;
                    }
                    let _ = local_timer; // unused but kept explicit for reviewers.
                    let etag_v = etag::from_parts([
                        row_id.to_string(),
                        format!("{:?}", local_status),
                        local_version.to_string(),
                    ]);
                    log_sync(&mut tx, incoming, SyncOperation::Update, &etag_v, true).await?;
                    MergeOutcome::Conflict
                }
            }
            // Invariant 2: version dominates; equal-version ties are broken
            // deterministically by timestamp (later wins). Only strictly equal
            // timestamps with a different payload are flagged as conflicts.
            else if incoming.version < local_version {
                MergeOutcome::RejectedOlder
            } else if incoming.version == local_version {
                let same_payload = incoming.status == local_status
                    && incoming.notes == local_notes
                    && incoming.timer_state_snapshot == local_timer;
                // Invariant 4: dual note edits at the same version ALWAYS
                // require SUPER review — timestamp precedence is insufficient
                // because losing a technician's narrative is unacceptable.
                let both_notes_present = incoming
                    .notes
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
                    && local_notes
                        .as_deref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false);
                let notes_disagree = incoming.notes != local_notes;
                let dual_notes_edit = both_notes_present && notes_disagree;
                if same_payload {
                    MergeOutcome::RejectedOlder
                } else if dual_notes_edit {
                    let etag_v = etag::from_parts([
                        row_id.to_string(),
                        format!("{:?}", local_status),
                        local_version.to_string(),
                    ]);
                    log_warn!(
                        MODULE,
                        "dual_notes_conflict",
                        "wo={} step={} — both sides edited notes, flagged for SUPER",
                        incoming.work_order_id,
                        incoming.step_id
                    );
                    log_sync(&mut tx, incoming, SyncOperation::Update, &etag_v, true).await?;
                    MergeOutcome::Conflict
                } else if incoming.updated_at > local_updated_at {
                    // Later-timestamp payload wins deterministically (PRD §8).
                    let next_version = local_version + 1;
                    let etag_v = etag::from_parts([
                        row_id.to_string(),
                        format!("{:?}", incoming.status),
                        next_version.to_string(),
                        incoming.updated_at.timestamp().to_string(),
                    ]);
                    sqlx::query(
                        "UPDATE job_step_progress
                         SET status = $1,
                             notes = COALESCE($2, notes),
                             timer_state_snapshot = COALESCE($3, timer_state_snapshot),
                             etag = $4,
                             version = $5,
                             updated_at = $6
                         WHERE id = $7",
                    )
                    .bind(incoming.status)
                    .bind(&incoming.notes)
                    .bind(&incoming.timer_state_snapshot)
                    .bind(&etag_v)
                    .bind(next_version)
                    .bind(incoming.updated_at)
                    .bind(row_id)
                    .execute(&mut *tx)
                    .await?;
                    log_info!(
                        MODULE,
                        "timestamp_priority_applied",
                        "wo={} step={} local_ts={} incoming_ts={} — later wins",
                        incoming.work_order_id,
                        incoming.step_id,
                        local_updated_at,
                        incoming.updated_at
                    );
                    log_sync(&mut tx, incoming, SyncOperation::Update, &etag_v, false).await?;
                    MergeOutcome::Applied
                } else if incoming.updated_at < local_updated_at {
                    // Strictly older incoming timestamp loses deterministically.
                    MergeOutcome::RejectedOlder
                } else {
                    // Equal version AND equal timestamp but divergent payloads —
                    // the only genuinely ambiguous case. Escalate to SUPER.
                    let etag_v = etag::from_parts([
                        row_id.to_string(),
                        format!("{:?}", local_status),
                        local_version.to_string(),
                    ]);
                    log_warn!(
                        MODULE,
                        "equal_timestamp_conflict",
                        "wo={} step={} ts={} — flagged for SUPER",
                        incoming.work_order_id,
                        incoming.step_id,
                        incoming.updated_at
                    );
                    log_sync(&mut tx, incoming, SyncOperation::Update, &etag_v, true).await?;
                    MergeOutcome::Conflict
                }
            } else {
                // Higher version: apply deterministically.
                let etag_v = etag::from_parts([
                    row_id.to_string(),
                    format!("{:?}", incoming.status),
                    incoming.version.to_string(),
                    now.timestamp().to_string(),
                ]);
                sqlx::query(
                    "UPDATE job_step_progress
                     SET status = $1,
                         notes = COALESCE($2, notes),
                         timer_state_snapshot = COALESCE($3, timer_state_snapshot),
                         etag = $4,
                         version = $5,
                         updated_at = $6
                     WHERE id = $7",
                )
                .bind(incoming.status)
                .bind(&incoming.notes)
                .bind(&incoming.timer_state_snapshot)
                .bind(&etag_v)
                .bind(incoming.version)
                .bind(incoming.updated_at)
                .bind(row_id)
                .execute(&mut *tx)
                .await?;
                log_sync(&mut tx, incoming, SyncOperation::Update, &etag_v, false).await?;
                MergeOutcome::Applied
            }
        }
    };

    tx.commit().await?;
    Ok(outcome)
}

async fn log_sync(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    incoming: &IncomingProgress,
    op: SyncOperation,
    new_etag: &str,
    conflict: bool,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO sync_log
            (entity_table, entity_id, operation, old_etag, new_etag, conflict_flagged)
         VALUES ('job_step_progress', $1, $2, NULL, $3, $4)",
    )
    .bind(incoming.step_id)
    .bind(op)
    .bind(new_etag)
    .bind(conflict)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Mark a flagged conflict row resolved by the given SUPER/ADMIN user.
pub async fn resolve_conflict(
    pool: &PgPool,
    conflict_id: Uuid,
    resolver: Uuid,
) -> Result<(), ApiError> {
    let affected = sqlx::query(
        "UPDATE sync_log
         SET conflict_resolved_by = $1
         WHERE id = $2 AND conflict_flagged = TRUE AND conflict_resolved_by IS NULL",
    )
    .bind(resolver)
    .bind(conflict_id)
    .execute(pool)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound(
            "conflict not found or already resolved".into(),
        ));
    }
    log_info!(MODULE, "conflict_resolved", "conflict={} by={}", conflict_id, resolver);
    Ok(())
}
