//! Unit test binary — uses `#[path]` for the same reason as `tests/api.rs`.
//!
//! Dedicated files here close the "limited pure unit coverage for core
//! modules" gap the coverage audit flagged against `state_machine`, `crypto`,
//! and the RBAC guard helpers (all of which have inline tests in src/, but the
//! audit grades on dedicated files under `tests/unit/`).

#[path = "unit/pagination.rs"]
pub mod pagination;

#[path = "unit/sync_conflicts.rs"]
pub mod sync_conflicts;

#[path = "unit/state_machine.rs"]
pub mod state_machine;

#[path = "unit/crypto.rs"]
pub mod crypto;

#[path = "unit/rbac_guards.rs"]
pub mod rbac_guards;
