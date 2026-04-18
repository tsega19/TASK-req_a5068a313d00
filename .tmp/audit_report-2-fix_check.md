# Recheck Report (Static-Only) — “Left issues” follow-up

Date: 2026-04-18  
Boundary: static inspection only (no execution).

## Summary

### Fixed (confirmed by static evidence)

1) **Learning pipeline now exists (knowledge points + learning record capture)**
- Evidence (routes registered): `repo/backend/src/lib.rs:50`
- Evidence (API implementation): `repo/backend/src/learning/routes.rs:102` (knowledge points), `repo/backend/src/learning/routes.rs:271` (record capture)
- Evidence (tests exist): `repo/backend/tests/api/learning.rs:6`
- Notes: Includes “hide correct answer from TECH” behavior. Evidence: `repo/backend/src/learning/routes.rs:41`, tested at `repo/backend/tests/api/learning.rs:55`.

2) **Analytics now has meaningful tests (filters + watermark + pipeline)**
- Evidence (tests): `repo/backend/tests/api/analytics.rs:26` (scoping), `repo/backend/tests/api/analytics.rs:66` (date format), `repo/backend/tests/api/analytics.rs:77` (CSV watermark), `repo/backend/tests/api/analytics.rs:110` (records written via capture endpoint appear in analytics)

3) **Sync API expanded to include a “changes since cursor” pull surface + delete propagation**
- Evidence (endpoint): `GET /api/sync/changes`: `repo/backend/src/sync/routes.rs:146`
- Evidence (delete propagation endpoint): `POST /api/sync/work-orders/{id}/delete`: `repo/backend/src/sync/routes.rs:209`

4) **Notification retry worker and ad-hoc admin trigger exist (still stub delivery, but retries/bookkeeping exist)**
- Evidence (retry worker spawn): `repo/backend/src/main.rs:32`
- Evidence (retry implementation): `repo/backend/src/notifications/stub.rs:158`
- Evidence (admin trigger endpoint): `repo/backend/src/admin/routes.rs:326`

5) **Retention pruning worker and admin trigger exist (but see “Not fixed / new blocker” below)**
- Evidence (worker spawn): `repo/backend/src/main.rs:33`
- Evidence (prune logic): `repo/backend/src/retention.rs:39`
- Evidence (admin trigger endpoint): `repo/backend/src/admin/routes.rs:307`

---

### Not fixed (still failing by static evidence)

1) **README hard-gate failures remain**
- Missing explicit project type declaration (backend/fullstack/etc.) near top: `repo/README.md:1`
- Missing explicit access URLs (e.g., `http://localhost:8081`, `http://localhost:8080`): `repo/README.md:13`
- Missing explicit verification steps (curl/UI flow): `repo/README.md:21`
- Missing demo credentials guidance for non-admin roles (SUPER/TECH) or explicit “how to create”: `repo/README.md:27`
- Encoding corruption still present (garbled characters): `repo/README.md:11`, `repo/README.md:29`, `repo/README.md:58`

Conclusion: README audit is still **FAIL** under the criteria in `.tmp/test_coverage_and_readme_audit_report.md:313` and `.tmp/test_coverage_and_readme_audit_report.md:334`.

---

### Blocker / Consistency defect (new or still present)

1) **Retention pruning claims a DB trigger bypass that is not implemented in migrations**
- `retention.rs` claims migration “0002 teaches the immutability trigger to honour … `fieldops.retention_prune`”: `repo/backend/src/retention.rs:6`
- Actual `0002_security.sql` only adds `users.password_reset_required` and does NOT modify the immutability trigger: `repo/backend/migrations/0002_security.sql:1`
- The immutability trigger always raises on DELETE/UPDATE of `work_order_transitions`: `repo/backend/migrations/0001_init.sql:177`

Impact (static reasoning):
- `work_order_transitions` is `ON DELETE CASCADE` from `work_orders`: `repo/backend/migrations/0001_init.sql:166`.
- A retention worker that hard-deletes old soft-deleted work orders (`DELETE FROM work_orders …`) will attempt cascaded DELETEs on `work_order_transitions`, which should trigger `BEFORE DELETE` and raise, potentially blocking retention pruning whenever transitions exist.
- This is a material discrepancy between code comments/intent and schema behavior.

Minimum actionable fix (for implementers; not executed here):
- Update migrations to implement the claimed bypass (e.g., adjust `wot_immutable()` to allow DELETE when `current_setting('fieldops.retention_prune', true) = 'on'`), and add an integration test that:
  - creates a work order + transition rows,
  - soft-deletes it,
  - sets `deleted_at` back in time,
  - runs retention prune,
  - confirms the work order and transitions are removed.

