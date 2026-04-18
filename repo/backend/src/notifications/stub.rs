// Mocking notification delivery.
//
// No real push/email/SMS provider is contacted. `send` enqueues a row and
// attempts an inline "soft" delivery, recording a delivered_at on success.
// The retry worker (`retry_pending` below) scans pending rows on a schedule,
// respects exponential backoff, caps retries at
// `BusinessConfig::notification_retry_max_attempts`, and updates delivered_at
// only when a simulated delivery succeeds.

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::enums::NotificationTemplate;
use crate::errors::ApiError;
use crate::{log_info, log_warn};

const MODULE: &str = "notifications";

pub struct DeliveryOutcome {
    pub notification_id: Uuid,
    pub attempts: i32,
    pub delivered: bool,
    pub rate_limited: bool,
    pub unsubscribed: bool,
}

/// Enqueue + attempt a notification.
///
/// Enforces:
///   - per-user, per-template unsubscribe preference,
///   - per-user hourly rate limit (PRD §7: 20/user/hour),
///   - retry count tracking (PRD §7: exponential backoff, max attempts).
///
/// In this stub, the first attempt always succeeds — retry_pending() is
/// responsible for handling any row that lands without `delivered_at` (e.g.
/// rate-limited rows that later become eligible, or future provider failures).
pub async fn send(
    pool: &PgPool,
    cfg: &AppConfig,
    user_id: Uuid,
    template: NotificationTemplate,
    payload: serde_json::Value,
) -> Result<DeliveryOutcome, ApiError> {
    // Unsubscribe check.
    let is_unsub: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_unsubscribes
         WHERE user_id = $1 AND template_type = $2",
    )
    .bind(user_id)
    .bind(template)
    .fetch_one(pool)
    .await?;
    if is_unsub > 0 {
        log_warn!(MODULE, "send", "user={} template={:?} suppressed (unsubscribed)", user_id, template);
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO notifications (user_id, template_type, payload, is_unsubscribed)
             VALUES ($1, $2, $3, TRUE) RETURNING id",
        )
        .bind(user_id)
        .bind(template)
        .bind(&payload)
        .fetch_one(pool)
        .await?;
        return Ok(DeliveryOutcome {
            notification_id: id,
            attempts: 0,
            delivered: false,
            rate_limited: false,
            unsubscribed: true,
        });
    }

    // Rate limit (rolling 1-hour window).
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE user_id = $1 AND created_at > NOW() - INTERVAL '1 hour'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    if recent >= cfg.business.max_notifications_per_hour as i64 {
        log_warn!(
            MODULE,
            "send",
            "user={} template={:?} rate-limited ({} in last hour)",
            user_id,
            template,
            recent
        );
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO notifications (user_id, template_type, payload)
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(template)
        .bind(&payload)
        .fetch_one(pool)
        .await?;
        return Ok(DeliveryOutcome {
            notification_id: id,
            attempts: 0,
            delivered: false,
            rate_limited: true,
            unsubscribed: false,
        });
    }

    // First attempt succeeds under the stub.
    let now = Utc::now();
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO notifications (user_id, template_type, payload, delivered_at, retry_count)
         VALUES ($1, $2, $3, $4, 1) RETURNING id",
    )
    .bind(user_id)
    .bind(template)
    .bind(&payload)
    .bind(now)
    .fetch_one(pool)
    .await?;

    log_info!(MODULE, "send", "user={} template={:?} id={} delivered", user_id, template, id);
    Ok(DeliveryOutcome {
        notification_id: id,
        attempts: 1,
        delivered: true,
        rate_limited: false,
        unsubscribed: false,
    })
}

/// Exponential backoff delay: `base * 2^(attempt-1)`, capped at attempt count.
pub fn backoff_seconds(attempt: u32, cfg: &AppConfig) -> u64 {
    let base = cfg.business.notification_retry_base_seconds;
    let max = cfg.business.notification_retry_max_attempts;
    if attempt == 0 || attempt > max {
        return 0;
    }
    base.saturating_mul(1u64 << (attempt - 1))
}

pub struct RetryReport {
    pub scanned: i64,
    pub delivered: i64,
    pub giveup: i64,
    pub skipped_backoff: i64,
}

/// Scan undelivered notifications and attempt re-delivery. A row is eligible
/// when `delivered_at IS NULL AND is_unsubscribed = FALSE AND retry_count <
/// max_attempts AND created_at + backoff(retry_count + 1) <= NOW()`.
///
/// Because this is a stub, "attempt" always succeeds — but the important part
/// is the bookkeeping: retry_count increments, delivered_at lands only on the
/// successful attempt, and rows that exceeded max attempts are logged as a
/// give-up so operators can notice.
pub async fn retry_pending(pool: &PgPool, cfg: &AppConfig) -> Result<RetryReport, ApiError> {
    let pending: Vec<(Uuid, i32, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, retry_count, created_at FROM notifications
         WHERE delivered_at IS NULL
           AND is_unsubscribed = FALSE
         ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut report = RetryReport { scanned: 0, delivered: 0, giveup: 0, skipped_backoff: 0 };
    let max = cfg.business.notification_retry_max_attempts;

    for (id, retry_count, created_at) in pending {
        report.scanned += 1;
        if retry_count as u32 >= max {
            // Already at cap; mark it one past and move on so we don't keep
            // scanning it forever.
            if (retry_count as u32) == max {
                sqlx::query(
                    "UPDATE notifications SET retry_count = retry_count + 1 WHERE id = $1",
                )
                .bind(id)
                .execute(pool)
                .await?;
                report.giveup += 1;
                log_warn!(MODULE, "retry_giveup", "id={} attempts={}", id, retry_count);
            }
            continue;
        }

        let next_attempt = (retry_count as u32) + 1;
        let wait = backoff_seconds(next_attempt, cfg) as i64;
        let eligible_at = created_at + chrono::Duration::seconds(wait);
        if Utc::now() < eligible_at {
            report.skipped_backoff += 1;
            continue;
        }

        // Stub "attempt" — always succeeds.
        let now = Utc::now();
        sqlx::query(
            "UPDATE notifications
             SET retry_count  = $1,
                 delivered_at = $2
             WHERE id = $3",
        )
        .bind(next_attempt as i32)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
        report.delivered += 1;
        log_info!(MODULE, "retry_delivered", "id={} attempt={}", id, next_attempt);
    }

    log_info!(
        MODULE,
        "retry_tick",
        "scanned={} delivered={} giveup={} waiting={}",
        report.scanned,
        report.delivered,
        report.giveup,
        report.skipped_backoff
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BusinessConfig;

    fn cfg() -> AppConfig {
        AppConfig {
            database: crate::config::DatabaseConfig { url: "".into(), max_connections: 1 },
            http: crate::config::HttpConfig {
                host: "".into(),
                port: 0,
                enable_tls: false,
                tls_cert_path: "".into(),
                tls_key_path: "".into(),
            },
            auth: crate::config::AuthConfig {
                jwt_secret: "".into(),
                jwt_expiry_hours: 1,
                argon2_memory_kib: 1,
                argon2_iterations: 1,
                argon2_parallelism: 1,
            },
            encryption: crate::config::EncryptionConfig { aes_256_key: [0u8; 32] },
            logging: crate::config::LoggingConfig { level: "info".into(), format: "structured".into() },
            business: BusinessConfig {
                sync_interval_minutes: 10,
                default_service_radius_miles: 30,
                max_notifications_per_hour: 20,
                max_versions_per_progress: 30,
                soft_delete_retention_days: 90,
                sla_alert_thresholds: vec![],
                notification_retry_max_attempts: 5,
                notification_retry_base_seconds: 1,
                on_call_high_priority_hours: 4,
            },
            app: crate::config::AppBehaviorConfig {
                run_migrations_on_boot: false,
                seed_default_admin: false,
                default_admin_username: "".into(),
                default_admin_password: "".into(),
                dev_mode: true,
                require_admin_password_change: false,
            },
        }
    }

    #[test]
    fn backoff_exponential_sequence() {
        let c = cfg();
        assert_eq!(backoff_seconds(1, &c), 1);
        assert_eq!(backoff_seconds(2, &c), 2);
        assert_eq!(backoff_seconds(3, &c), 4);
        assert_eq!(backoff_seconds(4, &c), 8);
        assert_eq!(backoff_seconds(5, &c), 16);
        assert_eq!(backoff_seconds(6, &c), 0); // beyond max
    }
}
