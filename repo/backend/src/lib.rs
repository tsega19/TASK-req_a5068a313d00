//! Crate root — declares every module, exposes the App wiring used by both
//! the binary and integration tests.

use actix_web::{web, HttpResponse};

#[path = "../config/mod.rs"]
pub mod config;

#[path = "../logging/mod.rs"]
pub mod logging;

pub mod admin;
pub mod analytics;
pub mod auth;
pub mod crypto;
pub mod db;
pub mod enums;
pub mod errors;
pub mod etag;
pub mod geo;
pub mod learning;
pub mod location;
pub mod me;
pub mod middleware;
pub mod notifications;
pub mod pagination;
pub mod processing_log;
pub mod recipes;
pub mod retention;
pub mod sla;
pub mod state_machine;
pub mod sync;
pub mod work_orders;

pub async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

/// Single source of route registration — shared by the live server and the
/// actix-web test harness.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health))
        .route("/api/health", web::get().to(health))
        .service(auth::routes::scope())
        .service(me::scope())
        .service(work_orders::scope())
        .service(recipes::recipes_scope())
        .service(recipes::steps_scope())
        .service(recipes::tip_cards_scope())
        .service(notifications::scope())
        .service(analytics::scope())
        .service(learning::knowledge_scope())
        .service(learning::records_scope())
        .service(sync::routes::scope())
        .service(location::scope())
        .service(admin::scope());
}

/// Startup: connect pool, apply migrations if enabled, seed the default admin.
pub async fn bootstrap(cfg: &config::AppConfig) -> anyhow::Result<sqlx::PgPool> {
    let pool = db::connect(cfg).await?;
    if cfg.app.run_migrations_on_boot {
        db::run_migrations(&pool).await?;
    }
    db::seed_default_admin(&pool, cfg).await?;
    Ok(pool)
}

/// Background sync ticker. Emits one log line per tick.
pub fn spawn_sync_ticker(pool: sqlx::PgPool, minutes: u64) {
    if minutes == 0 {
        log_warn!("boot", "sync_ticker", "disabled (interval=0)");
        return;
    }
    actix_web::rt::spawn(async move {
        let mut interval =
            actix_web::rt::time::interval(std::time::Duration::from_secs(minutes * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            match sync::trigger(&pool).await {
                Ok(report) => log_info!(
                    "boot",
                    "sync_ticker",
                    "tick ok wo_upd={} prog_upd={} conflicts={}",
                    report.work_orders_updated,
                    report.progress_updated,
                    report.conflicts_flagged
                ),
                Err(e) => log_error!("boot", "sync_ticker", "tick failed: {}", e),
            }
        }
    });
}

/// Background notification retry worker. Walks undelivered rows on a schedule,
/// increments retry_count up to `notification_retry_max_attempts`, and only
/// sets delivered_at on a successful attempt (PRD §7).
pub fn spawn_notification_retry_worker(pool: sqlx::PgPool, cfg: config::AppConfig) {
    let base = cfg.business.notification_retry_base_seconds.max(1);
    actix_web::rt::spawn(async move {
        let mut interval =
            actix_web::rt::time::interval(std::time::Duration::from_secs(base));
        interval.tick().await;
        loop {
            interval.tick().await;
            match notifications::stub::retry_pending(&pool, &cfg).await {
                Ok(r) => log_info!(
                    "boot",
                    "notification_retry",
                    "tick scanned={} delivered={} giveup={} waiting={}",
                    r.scanned,
                    r.delivered,
                    r.giveup,
                    r.skipped_backoff
                ),
                Err(e) => log_error!("boot", "notification_retry", "tick failed: {}", e),
            }
        }
    });
}

/// Background SLA alert worker. Scans work orders on a cadence and enqueues
/// in-app notifications when any configured threshold has been crossed (PRD §7).
/// Deduplication happens inside `sla::scan_and_alert`.
pub fn spawn_sla_alert_worker(pool: sqlx::PgPool, cfg: config::AppConfig) {
    if cfg.business.sla_alert_thresholds.is_empty() {
        log_warn!("boot", "sla_alert", "disabled (no thresholds configured)");
        return;
    }
    actix_web::rt::spawn(async move {
        // Scan every minute — cheap, and the dedup guard keeps notifications
        // one-shot per threshold per work order.
        let mut interval =
            actix_web::rt::time::interval(std::time::Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            match sla::scan_and_alert(&pool, &cfg).await {
                Ok(r) => log_info!(
                    "boot",
                    "sla_alert",
                    "tick scanned={} emitted={} deduped={}",
                    r.scanned,
                    r.alerts_emitted,
                    r.deduped
                ),
                Err(e) => log_error!("boot", "sla_alert", "tick failed: {}", e),
            }
        }
    });
}

/// Background retention pruning worker. Hard-deletes rows whose
/// `deleted_at` predates `soft_delete_retention_days` (PRD §7). Immutable
/// records such as `work_order_transitions` are never touched.
pub fn spawn_retention_worker(pool: sqlx::PgPool, cfg: config::AppConfig) {
    actix_web::rt::spawn(async move {
        // Runs daily — pruning is not time-critical.
        let mut interval =
            actix_web::rt::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            match retention::prune(&pool, &cfg).await {
                Ok(r) => log_info!(
                    "boot",
                    "retention",
                    "tick users={} work_orders={}",
                    r.users_pruned,
                    r.work_orders_pruned
                ),
                Err(e) => log_error!("boot", "retention", "tick failed: {}", e),
            }
        }
    });
}
