//! Append-only processing log (PRD §7): every state-changing user action
//! writes one row here so the system has a single immutable audit trail.
//!
//! The DB enforces immutability with a BEFORE UPDATE/DELETE trigger; this
//! module just owns the write helper and a lightweight read helper used by
//! the admin UI + tests.

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::log_warn;

/// Core action identifiers. Free-form strings are accepted for forward
/// compatibility, but these constants cover everything written from the
/// current codebase so greps stay meaningful.
pub mod actions {
    pub const WO_CREATE: &str = "work_order.create";
    pub const WO_TRANSITION: &str = "work_order.transition";
    pub const WO_DELETE: &str = "work_order.delete";
    pub const STEP_PROGRESS_UPSERT: &str = "step_progress.upsert";
    pub const CHECK_IN: &str = "check_in.create";
    pub const TRAIL_POINT: &str = "location_trail.append";
    pub const NOTIFICATION_READ: &str = "notification.read";
    pub const AUTH_LOGIN: &str = "auth.login";
    pub const AUTH_LOGOUT: &str = "auth.logout";
    pub const AUTH_CHANGE_PASSWORD: &str = "auth.change_password";
    pub const SLA_ALERT_EMITTED: &str = "sla.alert_emitted";
    // Admin user/branch management
    pub const USER_CREATE: &str = "admin.user.create";
    pub const USER_UPDATE: &str = "admin.user.update";
    pub const USER_DELETE: &str = "admin.user.delete";
    pub const BRANCH_CREATE: &str = "admin.branch.create";
    pub const BRANCH_UPDATE: &str = "admin.branch.update";
    // Learning authoring + delivery
    pub const KP_CREATE: &str = "knowledge_point.create";
    pub const KP_UPDATE: &str = "knowledge_point.update";
    pub const KP_DELETE: &str = "knowledge_point.delete";
    pub const LEARNING_RECORD: &str = "learning_record.create";
    pub const LEARNING_RECORD_REVIEW: &str = "learning_record.review";
    // Recipes / tip cards
    pub const TIP_CARD_CREATE: &str = "tip_card.create";
    pub const TIP_CARD_UPDATE: &str = "tip_card.update";
    // Self-service (me/*) mutations
    pub const ME_PRIVACY_SET: &str = "me.privacy.set";
    pub const ME_HOME_ADDRESS_SET: &str = "me.home_address.set";
    // Notifications
    pub const NOTIFICATION_UNSUBSCRIBE: &str = "notification.unsubscribe";
    // Admin-initiated operational actions (PRD §7 audit: every privileged
    // operator action must land in the immutable log).
    pub const ADMIN_SYNC_TRIGGER: &str = "admin.sync.trigger";
    pub const ADMIN_RETENTION_PRUNE: &str = "admin.retention.prune";
    pub const ADMIN_NOTIFICATIONS_RETRY: &str = "admin.notifications.retry";
    pub const ADMIN_SLA_SCAN: &str = "admin.sla.scan";
    // Sync operator actions
    pub const SYNC_CONFLICT_RESOLVE: &str = "sync.conflict.resolve";
    pub const SYNC_WO_DELETE_PUSH: &str = "sync.work_order.delete_push";
    // Automatic dispatch (write-time + periodic reroute, PRD §7).
    pub const WO_AUTO_DISPATCH: &str = "work_order.auto_dispatch";
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProcessingLogRow {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub entity_table: String,
    pub entity_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Write one row on a **best-effort** basis. Failures are logged but not
/// returned to the caller.
///
/// ⚠️ Prefer [`record_tx`] in any state-changing code path. The PRD §7 audit
/// guarantee ("every transition and user action writes an immutable
/// processing log") requires transactional atomicity: if the audit write
/// fails, the business write must roll back. `record` is retained only for
/// background observability writes where losing a row under DB outage is
/// preferable to blocking the whole operation (and where there is no paired
/// business write to roll back).
#[deprecated(
    since = "0.2.0",
    note = "use record_tx in state-changing paths for strict auditability"
)]
pub async fn record(
    pool: &PgPool,
    user_id: Option<Uuid>,
    action: &str,
    entity_table: &str,
    entity_id: Option<Uuid>,
    payload: serde_json::Value,
) {
    let res = sqlx::query(
        "INSERT INTO processing_log
            (user_id, action, entity_table, entity_id, payload)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(action)
    .bind(entity_table)
    .bind(entity_id)
    .bind(payload)
    .execute(pool)
    .await;
    if let Err(e) = res {
        log_warn!(
            "processing_log",
            "record",
            "failed to write audit row action={} entity={}/{:?}: {}",
            action,
            entity_table,
            entity_id,
            e
        );
    }
}

/// Transactional variant — participates in the caller's transaction so the
/// audit row lands atomically with the business write. Returns an error on
/// failure so the caller can roll back if they require strict auditing.
pub async fn record_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Option<Uuid>,
    action: &str,
    entity_table: &str,
    entity_id: Option<Uuid>,
    payload: serde_json::Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO processing_log
            (user_id, action, entity_table, entity_id, payload)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(action)
    .bind(entity_table)
    .bind(entity_id)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
