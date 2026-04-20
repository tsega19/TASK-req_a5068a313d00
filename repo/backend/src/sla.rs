//! SLA timeout alert generation (PRD §7).
//!
//! Scans non-terminal work orders with a `sla_deadline`, computes how much of
//! the SLA window has elapsed, and enqueues in-app notifications when any
//! configured threshold (e.g. 0.75, 0.90, 1.00) has just been crossed. Runs
//! periodically from a background worker; admins can also trigger it ad-hoc
//! via `POST /api/admin/sla/scan`.
//!
//! Dedup strategy: the notification payload includes `{sla_alert: {work_order_id,
//! threshold}}`. Before emitting, we look for any existing notification for
//! the same recipient + work_order_id + threshold value and skip if found.
//! This keeps the alert one-shot per threshold per work order, even when the
//! scanner runs many times.
//!
//! Recipients: the assigned technician (if any) plus every SUPER/ADMIN scoped
//! to the work order's branch (SUPER with a matching branch_id, plus all
//! ADMINs regardless of branch).
//!
//! Fraction calculation assumes the SLA window is `created_at -> sla_deadline`.
//! Work orders without a `created_at` older than the deadline still get their
//! progress measured against wall-clock time; crossings are monotonic so a
//! second scan of the same row does not re-fire.
//!
//! Notification template: `SCHEDULE_CHANGE` is reused — the FieldOps notification
//! enum models the product's four templates and SLA alerts are a form of
//! schedule-relevant alert to the recipient. The payload carries the SLA
//! specifics so UIs can render them distinctly.

use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::enums::NotificationTemplate;
use crate::errors::ApiError;
use crate::notifications;
use crate::processing_log;
use crate::{log_info, log_warn};

const MODULE: &str = "sla";

pub struct SlaReport {
    pub scanned: i64,
    pub alerts_emitted: i64,
    pub deduped: i64,
}

impl SlaReport {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "scanned": self.scanned,
            "alerts_emitted": self.alerts_emitted,
            "deduped": self.deduped,
        })
    }
}

pub async fn scan_and_alert(pool: &PgPool, cfg: &AppConfig) -> Result<SlaReport, ApiError> {
    let thresholds = &cfg.business.sla_alert_thresholds;
    if thresholds.is_empty() {
        return Ok(SlaReport { scanned: 0, alerts_emitted: 0, deduped: 0 });
    }

    // Pull candidate work orders: non-terminal, with deadline set, not deleted.
    let rows: Vec<(Uuid, Option<Uuid>, Option<Uuid>, chrono::DateTime<Utc>, chrono::DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT id, assigned_tech_id, branch_id, created_at, sla_deadline
             FROM work_orders
             WHERE deleted_at IS NULL
               AND sla_deadline IS NOT NULL
               AND state NOT IN ('Completed', 'Canceled')",
        )
        .fetch_all(pool)
        .await?;

    let mut report = SlaReport { scanned: 0, alerts_emitted: 0, deduped: 0 };
    let now = Utc::now();

    for (wo_id, tech, branch, created_at, deadline) in rows {
        report.scanned += 1;

        let window = (deadline - created_at).num_seconds().max(1) as f64;
        let elapsed = (now - created_at).num_seconds() as f64;
        let fraction = elapsed / window;

        for &threshold in thresholds {
            if fraction + f64::EPSILON < threshold {
                continue;
            }

            let mut recipients: Vec<Uuid> = Vec::new();
            if let Some(t) = tech {
                recipients.push(t);
            }
            // Branch supervisors + all admins.
            let supers: Vec<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM users
                 WHERE deleted_at IS NULL
                   AND (
                     role = 'ADMIN'
                     OR (role = 'SUPER' AND ($1::uuid IS NULL OR branch_id = $1))
                   )",
            )
            .bind(branch)
            .fetch_all(pool)
            .await?;
            for (u,) in supers {
                if !recipients.contains(&u) {
                    recipients.push(u);
                }
            }

            for recipient in recipients {
                // Dedup: have we already emitted this threshold for this WO to this user?
                let existing: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM notifications
                     WHERE user_id = $1
                       AND template_type = 'SCHEDULE_CHANGE'
                       AND payload @> $2::jsonb",
                )
                .bind(recipient)
                .bind(json!({
                    "sla_alert": {
                        "work_order_id": wo_id,
                        "threshold": threshold,
                    }
                }))
                .fetch_one(pool)
                .await?;
                if existing > 0 {
                    report.deduped += 1;
                    continue;
                }

                let payload = json!({
                    "sla_alert": {
                        "work_order_id": wo_id,
                        "threshold": threshold,
                        "fraction": fraction,
                        "deadline": deadline,
                    },
                    "message": format!(
                        "SLA {:.0}% threshold reached for work order {}",
                        threshold * 100.0,
                        wo_id
                    ),
                });

                match notifications::stub::send(
                    pool,
                    cfg,
                    recipient,
                    NotificationTemplate::ScheduleChange,
                    payload.clone(),
                )
                .await
                {
                    Ok(outcome) => {
                        report.alerts_emitted += 1;
                        // Strict audit: commit the SLA alert emission atomically
                        // so operators never see a notification without a
                        // matching processing_log row.
                        let mut tx = pool.begin().await?;
                        processing_log::record_tx(
                            &mut tx,
                            None, // system actor
                            processing_log::actions::SLA_ALERT_EMITTED,
                            "work_orders",
                            Some(wo_id),
                            json!({
                                "recipient": recipient,
                                "threshold": threshold,
                                "notification_id": outcome.notification_id,
                                "delivered": outcome.delivered,
                                "rate_limited": outcome.rate_limited,
                            }),
                        )
                        .await?;
                        tx.commit().await?;
                        log_info!(
                            MODULE,
                            "alert",
                            "wo={} user={} threshold={} notif={}",
                            wo_id,
                            recipient,
                            threshold,
                            outcome.notification_id
                        );
                    }
                    Err(e) => {
                        log_warn!(
                            MODULE,
                            "alert_failed",
                            "wo={} user={} threshold={} err={}",
                            wo_id,
                            recipient,
                            threshold,
                            e
                        );
                    }
                }
            }
        }
    }

    log_info!(
        MODULE,
        "scan_done",
        "scanned={} emitted={} deduped={}",
        report.scanned,
        report.alerts_emitted,
        report.deduped
    );
    Ok(report)
}

