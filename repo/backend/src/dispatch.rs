//! Automatic dispatch routing (PRD §7).
//!
//! Two stateful behaviors live here so routing is enforced, not merely
//! observed by the on-call queue view:
//!
//!   1. **Write-time dispatch.** When a HIGH/CRITICAL work order is
//!      created without an `assigned_tech_id`, [`dispatch_on_create_tx`]
//!      picks the best on-call TECH in the same branch and assigns
//!      them in the same transaction as the insert. A matching
//!      `processing_log` row lands so the automatic decision is
//!      auditable.
//!
//!   2. **Periodic reroute.** [`scan_and_reroute`] walks non-terminal
//!      HIGH/CRITICAL work orders that are inside the SLA-urgency
//!      window (`now > sla_deadline - on_call_high_priority_hours`)
//!      and either (a) have no `assigned_tech_id` or (b) are still in
//!      `Scheduled` — no work has started — and re-routes them to the
//!      best available on-call TECH. Terminal/in-flight work orders
//!      are left alone so a reroute can't trample a job that is
//!      genuinely being worked on.
//!
//! "Best on-call TECH" = the TECH user in the target branch with the
//! fewest non-terminal HIGH/CRITICAL work orders currently assigned.
//! Ties are broken by `users.id` for determinism. If the branch has
//! no technicians, the work order stays unassigned and the caller
//! decides how to surface that (the read-side queue view still shows
//! it so a supervisor can intervene).

use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::enums::Priority;
use crate::errors::ApiError;
use crate::processing_log;
use crate::{log_info, log_warn};

const MODULE: &str = "dispatch";

#[derive(Debug, Serialize)]
pub struct DispatchReport {
    pub scanned: i64,
    pub assigned: i64,
    pub rerouted: i64,
    pub no_tech_available: i64,
}

impl DispatchReport {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "scanned": self.scanned,
            "assigned": self.assigned,
            "rerouted": self.rerouted,
            "no_tech_available": self.no_tech_available,
        })
    }
}

/// Returns true for priorities the dispatch rule applies to. CRITICAL
/// work orders follow the same routing path as HIGH — the PRD §7 "on-call"
/// rule is about time-sensitive work, not priority strings.
pub fn is_dispatchable(p: Priority) -> bool {
    matches!(p, Priority::High | Priority::Critical)
}

/// Pick the best on-call TECH in `branch_id`, skipping any UUID in
/// `exclude` (used by the reroute path so we don't reassign a stuck
/// work order to the same technician that's already blocking it).
/// Runs inside a transaction so the caller's `FOR UPDATE` ordering
/// with the assigning UPDATE is preserved.
async fn select_on_call_tech_tx(
    tx: &mut Transaction<'_, Postgres>,
    branch_id: Uuid,
    exclude: Option<Uuid>,
) -> Result<Option<Uuid>, ApiError> {
    let picked: Option<(Uuid,)> = sqlx::query_as(
        "SELECT u.id
         FROM users u
         LEFT JOIN (
             SELECT assigned_tech_id, COUNT(*) AS load
             FROM work_orders
             WHERE deleted_at IS NULL
               AND assigned_tech_id IS NOT NULL
               AND state NOT IN ('Completed', 'Canceled')
               AND priority IN ('HIGH', 'CRITICAL')
             GROUP BY assigned_tech_id
         ) w ON w.assigned_tech_id = u.id
         WHERE u.deleted_at IS NULL
           AND u.role = 'TECH'
           AND u.branch_id = $1
           AND ($2::uuid IS NULL OR u.id <> $2)
         ORDER BY COALESCE(w.load, 0) ASC, u.id ASC
         LIMIT 1",
    )
    .bind(branch_id)
    .bind(exclude)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(picked.map(|(id,)| id))
}

/// Transactional write-time dispatch. Call this from `create_work_order`
/// right after the INSERT but before the transaction commits so the
/// assignment is atomic with the row creation.
///
/// Returns `Some(tech_id)` when a tech was picked, `None` when the
/// rule did not apply (wrong priority, no branch, or no eligible tech
/// in the branch). Callers should treat `None` as "leave as-is" —
/// the read-side queue will surface the unassigned job.
pub async fn dispatch_on_create_tx(
    tx: &mut Transaction<'_, Postgres>,
    wo_id: Uuid,
    branch_id: Option<Uuid>,
    priority: Priority,
    preassigned_tech: Option<Uuid>,
    triggered_by: Option<Uuid>,
) -> Result<Option<Uuid>, ApiError> {
    // Rule applies only when the rest of the system has not already
    // made a routing decision: priority must be dispatchable, the job
    // must be pinned to a branch, and the caller must not have
    // hand-assigned a tech.
    if !is_dispatchable(priority) || branch_id.is_none() || preassigned_tech.is_some() {
        return Ok(None);
    }
    let branch = branch_id.expect("checked above");

    let Some(tech_id) = select_on_call_tech_tx(tx, branch, None).await? else {
        log_warn!(
            MODULE,
            "no_tech_for_branch",
            "wo={} branch={} priority={:?} — left unassigned",
            wo_id,
            branch,
            priority
        );
        return Ok(None);
    };

    sqlx::query(
        "UPDATE work_orders
         SET assigned_tech_id = $1, updated_at = NOW()
         WHERE id = $2",
    )
    .bind(tech_id)
    .bind(wo_id)
    .execute(&mut **tx)
    .await?;

    processing_log::record_tx(
        tx,
        triggered_by,
        processing_log::actions::WO_AUTO_DISPATCH,
        "work_orders",
        Some(wo_id),
        json!({
            "reason": "create_high_priority",
            "branch_id": branch,
            "priority": format!("{:?}", priority),
            "assigned_tech_id": tech_id,
        }),
    )
    .await?;

    log_info!(
        MODULE,
        "dispatch_on_create",
        "wo={} branch={} priority={:?} -> tech={}",
        wo_id,
        branch,
        priority,
        tech_id
    );
    Ok(Some(tech_id))
}

/// Periodic reroute: scan non-terminal HIGH/CRITICAL work orders
/// approaching SLA and assign (or reassign) a tech when the existing
/// assignment is clearly not working out.
pub async fn scan_and_reroute(
    pool: &PgPool,
    cfg: &AppConfig,
) -> Result<DispatchReport, ApiError> {
    let hours = cfg.business.on_call_high_priority_hours;
    let mut report = DispatchReport {
        scanned: 0,
        assigned: 0,
        rerouted: 0,
        no_tech_available: 0,
    };

    // Candidates: near-SLA, non-terminal, dispatchable priority, and
    // either unassigned OR still in 'Scheduled' (no work started).
    // Branch-less work orders are skipped — routing needs a branch.
    let rows: Vec<(Uuid, Option<Uuid>, Uuid)> = sqlx::query_as(
        "SELECT id, assigned_tech_id, branch_id
         FROM work_orders
         WHERE deleted_at IS NULL
           AND branch_id IS NOT NULL
           AND sla_deadline IS NOT NULL
           AND state IN ('Scheduled')
           AND priority IN ('HIGH', 'CRITICAL')
           AND NOW() > sla_deadline - make_interval(hours => $1)",
    )
    .bind(hours as i32)
    .fetch_all(pool)
    .await?;
    report.scanned = rows.len() as i64;

    for (wo_id, current_tech, branch) in rows {
        let mut tx = pool.begin().await?;
        let picked = select_on_call_tech_tx(&mut tx, branch, current_tech).await?;
        let Some(new_tech) = picked else {
            report.no_tech_available += 1;
            tx.rollback().await?;
            continue;
        };

        if Some(new_tech) == current_tech {
            // Same tech is still the best option — no change, no audit row.
            tx.rollback().await?;
            continue;
        }

        sqlx::query(
            "UPDATE work_orders
             SET assigned_tech_id = $1, updated_at = NOW()
             WHERE id = $2 AND state = 'Scheduled' AND deleted_at IS NULL",
        )
        .bind(new_tech)
        .bind(wo_id)
        .execute(&mut *tx)
        .await?;

        let action = if current_tech.is_some() {
            "reroute_near_sla"
        } else {
            "assign_near_sla"
        };
        processing_log::record_tx(
            &mut tx,
            None, // system actor
            processing_log::actions::WO_AUTO_DISPATCH,
            "work_orders",
            Some(wo_id),
            json!({
                "reason": action,
                "branch_id": branch,
                "previous_tech": current_tech,
                "assigned_tech_id": new_tech,
                "scanned_at": Utc::now(),
            }),
        )
        .await?;
        tx.commit().await?;

        if current_tech.is_some() {
            report.rerouted += 1;
            log_info!(
                MODULE,
                "rerouted",
                "wo={} branch={} {:?} -> {}",
                wo_id,
                branch,
                current_tech,
                new_tech
            );
        } else {
            report.assigned += 1;
            log_info!(
                MODULE,
                "assigned",
                "wo={} branch={} -> {}",
                wo_id,
                branch,
                new_tech
            );
        }
    }

    log_info!(
        MODULE,
        "scan_done",
        "scanned={} assigned={} rerouted={} no_tech={}",
        report.scanned,
        report.assigned,
        report.rerouted,
        report.no_tech_available
    );
    Ok(report)
}
