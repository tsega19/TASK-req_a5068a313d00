//! Thin entry point — delegates to the lib crate's wiring.

use actix_web::{web, App, HttpServer};
use fieldops_backend::config::AppConfig;
use fieldops_backend::middleware::rbac::JwtAuth;
use fieldops_backend::middleware::request_log::RequestLog;
use fieldops_backend::{
    bootstrap, configure, logging, spawn_notification_retry_worker, spawn_retention_worker,
    spawn_sla_alert_worker, spawn_sync_ticker,
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let cfg = AppConfig::from_env()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    logging::init(&cfg.logging.level, &cfg.logging.format);
    fieldops_backend::log_info!(
        "boot",
        "startup",
        "FieldOps backend starting on {}:{} tls={}",
        cfg.http.host,
        cfg.http.port,
        cfg.http.enable_tls
    );

    let pool = bootstrap(&cfg)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    spawn_sync_ticker(pool.clone(), cfg.business.sync_interval_minutes);
    spawn_notification_retry_worker(pool.clone(), cfg.clone());
    spawn_retention_worker(pool.clone(), cfg.clone());
    spawn_sla_alert_worker(pool.clone(), cfg.clone());

    let cfg_data = web::Data::new(cfg.clone());
    let pool_data = web::Data::new(pool);
    let host = cfg.http.host.clone();
    let port = cfg.http.port;

    HttpServer::new(move || {
        App::new()
            .app_data(cfg_data.clone())
            .app_data(pool_data.clone())
            .wrap(RequestLog)
            .wrap(JwtAuth)
            .configure(configure)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
