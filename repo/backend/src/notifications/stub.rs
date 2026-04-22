// In-app notification delivery state machine (PRD §7).
//
// "Stub" is a slight misnomer: no external provider is contacted, but the
// delivery pipeline below exercises the full state machine — enqueue,
// simulated attempt (which can fail), backoff, retry, and give-up — so the
// retry/receipt semantics behave like a production pipeline and match the
// PRD guarantees.
//
// Failure simulation:
//   - Callers can request a deterministic failure by setting
//     `{"_simulate_failure": true}` anywhere in the payload; this is how
//     tests exercise the retry path.
//   - Additionally, any payload with `_simulate_failure_count: N` causes
//     the first N attempts to fail before success — covering both the
//     "first attempt fails, retry succeeds" and "max attempts give up"
//     behaviors without depending on wall-clock pseudo-randomness.
//   - When no marker is present, attempts succeed (the happy path used by
//     every production caller).

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

/// Inspect a payload for the deterministic-failure markers documented at the
/// top of this module. Returns `true` when the current attempt should fail.
/// `attempts_so_far` is the value the row will be bumped to if this attempt
/// fails — so `_simulate_failure_count: 2` fails attempts 1 and 2 and
/// succeeds on attempt 3.
fn should_simulate_failure(payload: &serde_json::Value, attempts_so_far: i32) -> bool {
    if payload
        .get("_simulate_failure")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(n) = payload
        .get("_simulate_failure_count")
        .and_then(|v| v.as_i64())
    {
        return (attempts_so_far as i64) <= n;
    }
    false
}

/// Attempt a single delivery for the given notification row. On success
/// updates `delivered_at` and bumps `retry_count`; on failure just bumps
/// `retry_count` and leaves `delivered_at` NULL. Returns `true` on success.
async fn attempt_delivery(
    pool: &PgPool,
    notification_id: Uuid,
    payload: &serde_json::Value,
    next_attempt: i32,
) -> Result<bool, ApiError> {
    if should_simulate_failure(payload, next_attempt) {
        sqlx::query(
            "UPDATE notifications SET retry_count = $1 WHERE id = $2",
        )
        .bind(next_attempt)
        .bind(notification_id)
        .execute(pool)
        .await?;
        log_warn!(
            MODULE,
            "delivery_failed",
            "id={} attempt={} simulated provider failure",
            notification_id,
            next_attempt
        );
        Ok(false)
    } else {
        let now = Utc::now();
        sqlx::query(
            "UPDATE notifications
             SET retry_count = $1, delivered_at = $2
             WHERE id = $3",
        )
        .bind(next_attempt)
        .bind(now)
        .bind(notification_id)
        .execute(pool)
        .await?;
        log_info!(
            MODULE,
            "delivery_ok",
            "id={} attempt={}",
            notification_id,
            next_attempt
        );
        Ok(true)
    }
}

/// Enqueue + attempt a notification.
///
/// Enforces:
///   - per-user, per-template unsubscribe preference,
///   - per-user hourly rate limit (PRD §7: 20/user/hour),
///   - retry count tracking (PRD §7: exponential backoff, max attempts).
///
/// The first attempt runs inline via [`attempt_delivery`]. When it fails
/// (either a simulated provider error or, in a real deployment, a transient
/// connection issue), the row stays `delivered_at = NULL` with an elevated
/// `retry_count` so [`retry_pending`] will pick it up after backoff.
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
        // Row is persisted as PENDING (no delivered_at, retry_count=0) so
        // retry_pending can pick it up once the hourly window rolls forward.
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

    // Insert PENDING first, then run one delivery attempt. This keeps the
    // row's history coherent if the attempt fails: caller can rely on the
    // retry worker to converge.
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO notifications (user_id, template_type, payload, retry_count)
         VALUES ($1, $2, $3, 0) RETURNING id",
    )
    .bind(user_id)
    .bind(template)
    .bind(&payload)
    .fetch_one(pool)
    .await?;

    let delivered = attempt_delivery(pool, id, &payload, 1).await?;
    log_info!(
        MODULE,
        "send",
        "user={} template={:?} id={} delivered={}",
        user_id,
        template,
        id,
        delivered
    );
    Ok(DeliveryOutcome {
        notification_id: id,
        attempts: 1,
        delivered,
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
    pub failed_again: i64,
    pub rate_limited_waiting: i64,
}

/// Scan undelivered notifications and attempt re-delivery. Eligibility rules:
///
///   - skip unsubscribed rows,
///   - skip rows still inside the hourly rate-limit window for the user
///     (they become eligible once the window rolls),
///   - skip rows whose created_at + backoff(retry_count + 1) hasn't elapsed,
///   - give up on rows that have already exhausted max_attempts.
///
/// Each attempt goes through [`attempt_delivery`] — which may succeed OR
/// simulate a failure deterministically based on payload markers — so the
/// retry path is genuinely exercised even without a real delivery provider.
pub async fn retry_pending(pool: &PgPool, cfg: &AppConfig) -> Result<RetryReport, ApiError> {
    let pending: Vec<(Uuid, Uuid, i32, chrono::DateTime<Utc>, serde_json::Value)> = sqlx::query_as(
        "SELECT id, user_id, retry_count, created_at, payload FROM notifications
         WHERE delivered_at IS NULL
           AND is_unsubscribed = FALSE
         ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut report = RetryReport {
        scanned: 0,
        delivered: 0,
        giveup: 0,
        skipped_backoff: 0,
        failed_again: 0,
        rate_limited_waiting: 0,
    };
    let max = cfg.business.notification_retry_max_attempts;

    for (id, user_id, retry_count, created_at, payload) in pending {
        report.scanned += 1;
        if retry_count as u32 >= max {
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

        // Honor the user's rolling rate limit — previously rate-limited rows
        // become eligible once the hour has passed.
        let still_rate_limited: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications
             WHERE user_id = $1
               AND delivered_at IS NOT NULL
               AND created_at > NOW() - INTERVAL '1 hour'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
        if still_rate_limited >= cfg.business.max_notifications_per_hour as i64 {
            report.rate_limited_waiting += 1;
            continue;
        }

        let next_attempt = (retry_count as u32) + 1;
        let wait = backoff_seconds(next_attempt, cfg) as i64;
        let eligible_at = created_at + chrono::Duration::seconds(wait);
        if Utc::now() < eligible_at {
            report.skipped_backoff += 1;
            continue;
        }

        let delivered = attempt_delivery(pool, id, &payload, next_attempt as i32).await?;
        if delivered {
            report.delivered += 1;
        } else {
            report.failed_again += 1;
        }
    }

    log_info!(
        MODULE,
        "retry_tick",
        "scanned={} delivered={} giveup={} waiting={} failed_again={} rate_limited_waiting={}",
        report.scanned,
        report.delivered,
        report.giveup,
        report.skipped_backoff,
        report.failed_again,
        report.rate_limited_waiting
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
                jwt_issuer: "fieldops-test".into(),
                jwt_audience: "fieldops-test".into(),
            },
            encryption: crate::config::EncryptionConfig { aes_256_key: [0u8; 32] },
            logging: crate::config::LoggingConfig { level: "info".into(), format: "structured".into() },
            business: BusinessConfig {
                sync_interval_minutes: 10,
                default_service_radius_miles: 30,
                max_notifications_per_hour: 20,
                max_versions_per_record: 30,
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
                allow_geocode_fallback: true,
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

    #[test]
    fn simulate_failure_flag_recognized() {
        let payload = serde_json::json!({"_simulate_failure": true});
        assert!(should_simulate_failure(&payload, 1));
        assert!(should_simulate_failure(&payload, 9));
    }

    #[test]
    fn simulate_failure_count_bounds_fails() {
        let payload = serde_json::json!({"_simulate_failure_count": 2});
        assert!(should_simulate_failure(&payload, 1));
        assert!(should_simulate_failure(&payload, 2));
        assert!(!should_simulate_failure(&payload, 3));
    }
}
