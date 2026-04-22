# Re-Audit Fix Verification Report (Updated)

Source baseline: `.tmp/audit_report-1.md`  
Updated target: `.tmp/audit_report-3_fix.md`  
Audit mode: Static-only (no runtime execution, no tests run)

## 1) Overall Result
- **Status: Partial Pass (Improved)**
- **Summary:** The previously reported **Blocker** and one **High** issue are now statically addressed in code. The remaining version-retention issue is **partially fixed** (generic framework implemented), with residual scope/coverage caveats.

## 2) Issue-by-Issue Re-Verification

### Issue A (Blocker): Sync change-log uses wrong entity key for step progress
- Previous status: Not fixed
- **Current status: FIXED (static evidence)**
- Evidence:
  - Merge path now captures inserted progress row PK via `RETURNING id`: `repo/backend/src/sync/merge.rs:113`
  - `log_sync` now accepts and binds `progress_row_id` (not `step_id`): `repo/backend/src/sync/merge.rs:367`, `repo/backend/src/sync/merge.rs:383`
  - Reader still joins on `p.id = s.entity_id` (now aligned): `repo/backend/src/sync/routes.rs:232`, `repo/backend/src/sync/routes.rs:267`
- Conclusion: Writer/reader entity-key mismatch root cause has been corrected in source.
- Caveat: Existing tests still contain legacy assumptions in at least one assertion path and were not executed in this audit.

### Issue B (High): Automatic dispatch routing rule not implemented as stateful behavior
- Previous status: Not fixed
- **Current status: FIXED (static evidence)**
- Evidence:
  - Write-time dispatch on create is now invoked transactionally: `repo/backend/src/work_orders/routes.rs:299`
  - Stateful dispatch implementation exists (`dispatch_on_create_tx`): `repo/backend/src/dispatch.rs:113`
  - Periodic reroute worker exists (`scan_and_reroute`): `repo/backend/src/dispatch.rs:182`
  - Worker is started at boot: `repo/backend/src/main.rs:35`, `repo/backend/src/lib.rs:160`
  - Admin trigger endpoint for dispatch scan exists: `repo/backend/src/admin/routes.rs:645`, `repo/backend/src/admin/routes.rs:682`
- Conclusion: Dispatch is no longer read-only/observational; stateful routing logic now exists.

### Issue C (High): Historical version retention limited to step-progress only
- Previous status: Not fixed
- **Current status: PARTIALLY FIXED (static evidence)**
- Evidence of fix progress:
  - New generic `record_versions` migration/table + immutability trigger: `repo/backend/migrations/0006_record_versions.sql:17`, `repo/backend/migrations/0006_record_versions.sql:52`
  - Generic versioning helper module added: `repo/backend/src/versions.rs:52`
  - Shared retention cap config moved to generic key (`max_versions_per_record`): `repo/backend/config/mod.rs:88`, `repo/backend/config/mod.rs:249`
  - Work-order transition path snapshots to generic store: `repo/backend/src/work_orders/routes.rs:441`
  - Step-progress update path also snapshots to generic store: `repo/backend/src/work_orders/progress.rs:131`
  - Admin read surface for generic versions: `repo/backend/src/admin/routes.rs:615`, `repo/backend/src/admin/routes.rs:683`
- Residual gap:
  - Static evidence currently confirms adoption for work orders and step progress; broader adoption across all mutable entities is not yet evident.
- Conclusion: Major architectural fix landed, but requirement-fit breadth remains only partially confirmed statically.

## 3) Delta vs Previous `audit_report-3_fix.md`
- Sync key mismatch: **NOT FIXED -> FIXED**
- Dispatch routing behavior: **NOT FIXED -> FIXED**
- Version-retention scope: **NOT FIXED -> PARTIALLY FIXED**

## 4) Final Re-Audit Judgment
- The project has materially improved against the previously cited defects.
- Current static judgment for the prior three findings: **2 Fixed, 1 Partially Fixed**.
- Remaining work is primarily breadth/completeness and regression-proofing (tests), not the original blocker-level root causes.
