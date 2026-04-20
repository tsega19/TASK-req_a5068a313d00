//! API integration test binary.
//!
//! Integration-test crate roots (`tests/*.rs`) use `main.rs`-style module
//! resolution — `mod common;` alone would look for `tests/common.rs`. The
//! `#[path]` attributes below point each sub-module at its actual file in
//! `tests/api/`.

#[path = "api/common.rs"]
pub mod common;

#[path = "api/admin.rs"]
pub mod admin;
#[path = "api/analytics.rs"]
pub mod analytics;
#[path = "api/audit_log.rs"]
pub mod audit_log;
#[path = "api/auth.rs"]
pub mod auth;
#[path = "api/geocoding.rs"]
pub mod geocoding;
#[path = "api/learning.rs"]
pub mod learning;
#[path = "api/location.rs"]
pub mod location;
#[path = "api/me.rs"]
pub mod me;
#[path = "api/notifications.rs"]
pub mod notifications;
#[path = "api/rbac.rs"]
pub mod rbac;
#[path = "api/recipes.rs"]
pub mod recipes;
#[path = "api/retention.rs"]
pub mod retention;
#[path = "api/sla.rs"]
pub mod sla;
#[path = "api/sync.rs"]
pub mod sync;
#[path = "api/work_orders.rs"]
pub mod work_orders;
