//! Work order CRUD, state transitions, timeline, on-call queue.
//! RBAC enforced twice: the JwtAuth middleware guarantees an authenticated
//! user is present; each handler re-checks via `require_role` /
//! `require_any_role` and an object-level visibility filter.

use actix_web::{delete, get, http::header, post, put, web, HttpRequest, HttpResponse};
use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::config::AppConfig;
use crate::enums::{Priority, WorkOrderState};
use crate::errors::ApiError;
use crate::etag;
use crate::geo::haversine_miles;
use crate::location::geocode_stub;
use crate::middleware::rbac::{require_any_role, require_branch, AuthedUser};
use crate::pagination::{PageParams, Paginated};
use crate::processing_log;
use crate::state_machine::{allowed_transition, TransitionContext};
use crate::work_orders::models::{
    CreateWorkOrder, StateTransitionRequest, WorkOrder, WorkOrderTransition,
};
use crate::work_orders::progress;
use crate::{log_info, log_warn};

/// Parse and enforce the `If-Match` header (audit-2 High #3).
///
/// `actual_etag` is the ETag of the resource as currently stored. The client
/// may send the value either bare (`deadbeef`) or quoted (`"deadbeef"`); RFC
/// 7232 allows both. A missing or mismatched header returns
/// `412 Precondition Failed` — the client must refetch and retry.
pub(crate) fn require_if_match(req: &HttpRequest, actual_etag: Option<&str>) -> Result<(), ApiError> {
    let header_val = req
        .headers()
        .get(header::IF_MATCH)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let client = match header_val {
        Some(v) => v,
        None => {
            return Err(ApiError::PreconditionFailed(
                "If-Match header is required for this mutation".into(),
            ));
        }
    };
    let normalized = client.trim_start_matches("W/").trim_matches('"');
    match actual_etag {
        Some(current) if current == normalized => Ok(()),
        Some(_) => Err(ApiError::PreconditionFailed(
            "If-Match value is stale; refetch the resource".into(),
        )),
        None => Err(ApiError::PreconditionFailed(
            "resource has no ETag to match against".into(),
        )),
    }
}

/// Decide whether a work order should be auto-routed into the on-call queue.
///
/// PRD §7 / audit-2 High #2: a HIGH-priority work order whose SLA deadline is
/// within `ON_CALL_HIGH_PRIORITY_HOURS` of now MUST be routed to the on-call
/// queue automatically. Completed/Canceled orders are never on-call.
pub(crate) fn should_be_on_call(
    priority: Priority,
    state: WorkOrderState,
    sla_deadline: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
    hours: i64,
) -> bool {
    if state.is_terminal() || priority != Priority::High {
        return false;
    }
    match sla_deadline {
        Some(d) => d - now <= chrono::Duration::hours(hours),
        None => false,
    }
}

const MODULE: &str = "work_orders";

// -----------------------------------------------------------------------------
// List
// -----------------------------------------------------------------------------
#[get("")]
pub async fn list_work_orders(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<PageParams>,
) -> Result<HttpResponse, ApiError> {
    let params = q.into_inner();
    let (offset, limit) = params.offset_limit();

    let (rows, total): (Vec<WorkOrder>, i64) = match user.role() {
        Role::Tech => {
            let uid = user.user_id();
            let rows = sqlx::query_as::<_, WorkOrder>(
                "SELECT * FROM work_orders
                 WHERE deleted_at IS NULL AND assigned_tech_id = $1
                 ORDER BY priority DESC, sla_deadline ASC NULLS LAST, created_at DESC
                 OFFSET $2 LIMIT $3",
            )
            .bind(uid)
            .bind(offset)
            .bind(limit)
            .fetch_all(pool.get_ref())
            .await?;
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM work_orders WHERE deleted_at IS NULL AND assigned_tech_id = $1",
            )
            .bind(uid)
            .fetch_one(pool.get_ref())
            .await?;
            (rows, total)
        }
        Role::Super => {
            // Fail-closed: a SUPER with no branch claim cannot widen to every
            // branch's work orders (audit AR-1 High).
            let branch = require_branch(&user)?;
            let rows = sqlx::query_as::<_, WorkOrder>(
                "SELECT * FROM work_orders
                 WHERE deleted_at IS NULL AND branch_id = $1
                 ORDER BY priority DESC, sla_deadline ASC NULLS LAST, created_at DESC
                 OFFSET $2 LIMIT $3",
            )
            .bind(branch)
            .bind(offset)
            .bind(limit)
            .fetch_all(pool.get_ref())
            .await?;
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM work_orders
                 WHERE deleted_at IS NULL AND branch_id = $1",
            )
            .bind(branch)
            .fetch_one(pool.get_ref())
            .await?;
            (rows, total)
        }
        Role::Admin => {
            let rows = sqlx::query_as::<_, WorkOrder>(
                "SELECT * FROM work_orders
                 WHERE deleted_at IS NULL
                 ORDER BY priority DESC, sla_deadline ASC NULLS LAST, created_at DESC
                 OFFSET $1 LIMIT $2",
            )
            .bind(offset)
            .bind(limit)
            .fetch_all(pool.get_ref())
            .await?;
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM work_orders WHERE deleted_at IS NULL")
                    .fetch_one(pool.get_ref())
                    .await?;
            (rows, total)
        }
    };

    log_info!(MODULE, "list", "user={} role={} count={}", user.user_id(), user.role(), rows.len());
    Ok(HttpResponse::Ok().json(Paginated::new(rows, params, total)))
}

// -----------------------------------------------------------------------------
// On-call queue (SUPER / ADMIN) — registered BEFORE /{id} to avoid path clash.
// -----------------------------------------------------------------------------
#[get("/on-call-queue")]
pub async fn on_call_queue(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    _cfg: web::Data<AppConfig>,
) -> Result<HttpResponse, ApiError> {
    require_any_role(&user, &[Role::Super, Role::Admin])?;
    // PRD supervisor-scope rule: SUPER sees only their own branch; ADMIN is
    // global. The `$1::uuid IS NULL` arm covers the ADMIN case without a
    // second query path.
    let scope_branch: Option<Uuid> = match user.role() {
        // SUPER without a branch assignment would otherwise gain cross-branch
        // visibility; fail-close per audit AR-1 High.
        Role::Super => Some(require_branch(&user)?),
        Role::Admin => None,
        Role::Tech => unreachable!("require_any_role above rejects TECH"),
    };
    // Audit-2 High #2 fix: read the persisted routing decision instead of
    // recomputing the predicate on every query. The `on_call` column is
    // written on create + transition, so the queue is an actual routing
    // durable state rather than a filter that re-evaluates cold.
    let rows = sqlx::query_as::<_, WorkOrder>(
        "SELECT * FROM work_orders
         WHERE deleted_at IS NULL
           AND on_call = TRUE
           AND ($1::uuid IS NULL OR branch_id = $1)
         ORDER BY sla_deadline ASC NULLS LAST",
    )
    .bind(scope_branch)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "on_call_queue", "user={} role={} count={}", user.user_id(), user.role(), rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

// -----------------------------------------------------------------------------
// Get one
// -----------------------------------------------------------------------------
#[get("/{id}")]
pub async fn get_work_order(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let wo = load_visible(&pool, &user, id).await?;
    log_info!(MODULE, "get", "user={} wo={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(wo))
}

// -----------------------------------------------------------------------------
// Create (SUPER / ADMIN)
// -----------------------------------------------------------------------------
#[post("")]
pub async fn create_work_order(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    body: web::Json<CreateWorkOrder>,
) -> Result<HttpResponse, ApiError> {
    require_any_role(&user, &[Role::Super, Role::Admin])?;
    let mut req = body.into_inner();
    if req.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title required".into()));
    }

    // Offline ZIP+4 normalization (PRD §6). When the caller supplies a
    // free-form address, run it through the bundled geocoder and persist the
    // canonical form + coordinates. Explicit lat/lng in the request override
    // the geocoded coords so an operator can pin a precise location.
    if let Some(ref raw) = req.location_address_norm.clone() {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let g = geocode_stub::geocode(trimmed);
            // Strict mode: reject unknown addresses instead of silently
            // storing synthetic hash-derived coordinates, which would skew
            // branch-radius checks and location-trail analytics.
            if !g.from_index && !cfg.app.allow_geocode_fallback {
                return Err(ApiError::BadRequest(format!(
                    "address '{}' not found in bundled ZIP+4 index — supply a canonical ZIP+4 or explicit lat/lng",
                    trimmed
                )));
            }
            req.location_address_norm = Some(g.address_norm);
            if req.location_lat.is_none() {
                req.location_lat = Some(g.lat);
            }
            if req.location_lng.is_none() {
                req.location_lng = Some(g.lng);
            }
            log_info!(
                MODULE,
                "geocode",
                "input='{}' from_index={}",
                trimmed,
                g.from_index
            );
        }
    }

    // Radius validation (PRD §7) when both coordinates and a branch are given.
    if let (Some(lat), Some(lng), Some(branch_id)) = (req.location_lat, req.location_lng, req.branch_id) {
        let branch: Option<(Option<f64>, Option<f64>, i32)> = sqlx::query_as(
            "SELECT lat, lng, service_radius_miles FROM branches WHERE id = $1",
        )
        .bind(branch_id)
        .fetch_optional(pool.get_ref())
        .await?;
        match branch {
            Some((Some(blat), Some(blng), radius)) => {
                let d = haversine_miles(lat, lng, blat, blng);
                if d > radius as f64 {
                    return Err(ApiError::BadRequest(format!(
                        "job location is {:.1}mi from branch — exceeds {}mi radius",
                        d, radius
                    )));
                }
            }
            Some(_) => {} // branch exists but has no coordinates — skip check
            None => return Err(ApiError::BadRequest("branch not found".into())),
        }
    }

    let priority = req.priority.unwrap_or(crate::enums::Priority::Normal);
    let id = Uuid::new_v4();
    let now = Utc::now();
    let etag_v = etag::from_parts([
        id.to_string(),
        req.title.clone(),
        format!("{:?}", priority),
        now.timestamp().to_string(),
    ]);
    // Automatic on-call routing (audit-2 High #2): evaluate the rule on
    // create and persist the routing decision so the on-call queue reflects
    // durable state.
    let on_call = should_be_on_call(
        priority,
        WorkOrderState::Scheduled,
        req.sla_deadline,
        now,
        cfg.business.on_call_high_priority_hours,
    );

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO work_orders (id, title, description, priority, state,
            assigned_tech_id, branch_id, sla_deadline, recipe_id,
            location_address_norm, location_lat, location_lng,
            etag, version_count, on_call, created_at, updated_at)
         VALUES ($1,$2,$3,$4,'Scheduled',$5,$6,$7,$8,$9,$10,$11,$12,1,$13,$14,$14)",
    )
    .bind(id)
    .bind(&req.title)
    .bind(&req.description)
    .bind(priority)
    .bind(req.assigned_tech_id)
    .bind(req.branch_id)
    .bind(req.sla_deadline)
    .bind(req.recipe_id)
    .bind(&req.location_address_norm)
    .bind(req.location_lat)
    .bind(req.location_lng)
    .bind(&etag_v)
    .bind(on_call)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Record the initial (null -> Scheduled) transition for timeline completeness.
    sqlx::query(
        "INSERT INTO work_order_transitions (work_order_id, from_state, to_state, triggered_by, required_fields, notes)
         VALUES ($1, NULL, 'Scheduled', $2, '{}'::jsonb, 'initial')",
    )
    .bind(id)
    .bind(user.user_id())
    .execute(&mut *tx)
    .await?;

    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::WO_CREATE,
        "work_orders",
        Some(id),
        json!({
            "title": req.title,
            "priority": format!("{:?}", priority),
            "assigned_tech_id": req.assigned_tech_id,
            "branch_id": req.branch_id,
            "location_address_norm": req.location_address_norm,
            "on_call": on_call,
        }),
    )
    .await?;
    if on_call {
        // Closed-loop audit of the auto-routing decision, distinct from
        // the WO_CREATE row so a dashboard can count routing events.
        processing_log::record_tx(
            &mut tx,
            Some(user.user_id()),
            processing_log::actions::WO_ON_CALL_ROUTED,
            "work_orders",
            Some(id),
            json!({
                "trigger": "create",
                "priority": format!("{:?}", priority),
                "sla_deadline": req.sla_deadline,
                "window_hours": cfg.business.on_call_high_priority_hours,
            }),
        )
        .await?;
    }
    tx.commit().await?;

    // Silence the unused-config warning in the unlikely path where radius check
    // was skipped; cfg is re-used by background ticker via state machine logic.
    let _ = &cfg.business.default_service_radius_miles;

    let wo = sqlx::query_as::<_, WorkOrder>("SELECT * FROM work_orders WHERE id = $1")
        .bind(id)
        .fetch_one(pool.get_ref())
        .await?;

    log_info!(MODULE, "create", "user={} wo={} priority={:?}", user.user_id(), id, priority);
    Ok(HttpResponse::Created().json(wo))
}

// -----------------------------------------------------------------------------
// State transition
// -----------------------------------------------------------------------------
#[put("/{id}/state")]
pub async fn transition_state(
    http_req: HttpRequest,
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    path: web::Path<Uuid>,
    body: web::Json<StateTransitionRequest>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let req = body.into_inner();
    let wo = load_visible(&pool, &user, id).await?;

    // If-Match precondition (audit-2 High #3 / PRD §8): concurrent editors
    // must refetch on stale state. Check BEFORE any business validation so
    // a 412 surfaces regardless of payload correctness.
    require_if_match(&http_req, wo.etag.as_deref())?;

    if wo.state.is_terminal() {
        return Err(ApiError::Conflict(format!(
            "work order is terminal ({:?})",
            wo.state
        )));
    }

    allowed_transition(wo.state, req.to_state, user.role())?;

    // TECH must own the work order for any transition they perform.
    if matches!(user.role(), Role::Tech) && wo.assigned_tech_id != Some(user.user_id()) {
        return Err(ApiError::Forbidden("not assigned to this work order".into()));
    }

    // Arrival check-in validation: both presence and radius.
    let (arrival_present, arrival_within_radius) = if req.to_state == WorkOrderState::OnSite {
        let arrival_lat_lng: Option<(Option<f64>, Option<f64>)> = sqlx::query_as(
            "SELECT lat, lng FROM check_ins
             WHERE work_order_id = $1 AND type = 'ARRIVAL'
             ORDER BY recorded_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool.get_ref())
        .await?;

        let branch_coords: Option<(Option<f64>, Option<f64>, i32)> =
            if let Some(branch_id) = wo.branch_id {
                sqlx::query_as(
                    "SELECT lat, lng, service_radius_miles FROM branches WHERE id = $1",
                )
                .bind(branch_id)
                .fetch_optional(pool.get_ref())
                .await?
            } else {
                None
            };

        match (arrival_lat_lng, branch_coords) {
            (Some((Some(lat), Some(lng))), Some((Some(blat), Some(blng), radius))) => {
                (true, haversine_miles(lat, lng, blat, blng) <= radius as f64)
            }
            (Some(_), _) => (true, true), // best-effort when branch coords missing
            (None, _) => (false, false),
        }
    } else {
        (false, true)
    };

    // Departure check-in + step gate for Completed.
    let (departure_present, all_steps_completed) = if req.to_state == WorkOrderState::Completed {
        let dep: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM check_ins WHERE work_order_id = $1 AND type = 'DEPARTURE'",
        )
        .bind(id)
        .fetch_one(pool.get_ref())
        .await?;
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_step_progress
             WHERE work_order_id = $1 AND status <> 'Completed'",
        )
        .bind(id)
        .fetch_one(pool.get_ref())
        .await?;
        let expected: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM recipe_steps WHERE recipe_id = (
                SELECT recipe_id FROM work_orders WHERE id = $1
             )",
        )
        .bind(id)
        .fetch_one(pool.get_ref())
        .await?;
        let total_progress: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_step_progress WHERE work_order_id = $1",
        )
        .bind(id)
        .fetch_one(pool.get_ref())
        .await?;
        let gate = pending == 0 && total_progress >= expected && expected > 0;
        (dep > 0, gate)
    } else {
        (false, true)
    };

    let ctx = TransitionContext {
        notes: req.notes.clone(),
        lat: req.lat,
        lng: req.lng,
        arrival_check_in_present: arrival_present,
        arrival_within_radius,
        departure_check_in_present: departure_present,
        all_steps_completed,
    };
    ctx.validate_required(wo.state, req.to_state)?;

    // Apply transition inside a transaction so the state change and the
    // immutable transition log row land atomically.
    let mut tx = pool.begin().await?;
    let new_etag = etag::from_parts([
        id.to_string(),
        format!("{:?}", req.to_state),
        Utc::now().timestamp().to_string(),
    ]);
    // Re-evaluate auto on-call routing against the *target* state + current
    // SLA/priority (audit-2 High #2). Terminal states clear the flag; HIGH +
    // tight SLA keep or raise it. Recording the prior value lets us emit a
    // routing audit row only on transitions.
    let next_on_call = should_be_on_call(
        wo.priority,
        req.to_state,
        wo.sla_deadline,
        Utc::now(),
        cfg.business.on_call_high_priority_hours,
    );
    let on_call_changed = next_on_call != wo.on_call;
    sqlx::query(
        "UPDATE work_orders
         SET state = $1,
             etag = $2,
             version_count = version_count + 1,
             on_call = $3,
             updated_at = NOW()
         WHERE id = $4",
    )
    .bind(req.to_state)
    .bind(&new_etag)
    .bind(next_on_call)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let required_fields = json!({
        "notes": req.notes.is_some(),
        "lat": req.lat,
        "lng": req.lng,
    });
    sqlx::query(
        "INSERT INTO work_order_transitions
            (work_order_id, from_state, to_state, triggered_by, required_fields, notes)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(format!("{:?}", wo.state))
    .bind(format!("{:?}", req.to_state))
    .bind(user.user_id())
    .bind(required_fields)
    .bind(&req.notes)
    .execute(&mut *tx)
    .await?;

    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::WO_TRANSITION,
        "work_orders",
        Some(id),
        json!({
            "from": format!("{:?}", wo.state),
            "to": format!("{:?}", req.to_state),
            "lat": req.lat,
            "lng": req.lng,
            "notes_present": req.notes.is_some(),
            "on_call": next_on_call,
        }),
    )
    .await?;
    if on_call_changed {
        // Auto-routing audit row — separate from WO_TRANSITION so the
        // routing event is individually addressable in the processing log.
        processing_log::record_tx(
            &mut tx,
            Some(user.user_id()),
            processing_log::actions::WO_ON_CALL_ROUTED,
            "work_orders",
            Some(id),
            json!({
                "trigger": "transition",
                "previous_on_call": wo.on_call,
                "on_call": next_on_call,
                "state": format!("{:?}", req.to_state),
                "priority": format!("{:?}", wo.priority),
                "sla_deadline": wo.sla_deadline,
                "window_hours": cfg.business.on_call_high_priority_hours,
            }),
        )
        .await?;
    }
    tx.commit().await?;

    log_info!(
        MODULE,
        "transition",
        "user={} wo={} {:?} -> {:?}",
        user.user_id(),
        id,
        wo.state,
        req.to_state
    );

    // Fan out a templated CANCELLATION notification to the assigned technician
    // when a work order is canceled — satisfies PRD §7 event coverage. Runs
    // best-effort after the transition has already committed, so a transient
    // notification error never rolls back the state change.
    if matches!(req.to_state, WorkOrderState::Canceled) {
        if let Some(tech_id) = wo.assigned_tech_id {
            let payload = serde_json::json!({
                "work_order_id": id,
                "title": wo.title,
                "canceled_by": user.user_id(),
                "notes": req.notes,
            });
            if let Err(e) = crate::notifications::stub::send(
                pool.get_ref(),
                cfg.get_ref(),
                tech_id,
                crate::enums::NotificationTemplate::Cancellation,
                payload,
            )
            .await
            {
                log_warn!(MODULE, "cancellation_notify_failed", "wo={} err={}", id, e);
            }
        }
    }

    let wo_new = sqlx::query_as::<_, WorkOrder>("SELECT * FROM work_orders WHERE id = $1")
        .bind(id)
        .fetch_one(pool.get_ref())
        .await?;
    Ok(HttpResponse::Ok().json(wo_new))
}

// -----------------------------------------------------------------------------
// Timeline (immutable transition log)
// -----------------------------------------------------------------------------
#[get("/{id}/timeline")]
pub async fn timeline(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let _wo = load_visible(&pool, &user, id).await?;
    let rows = sqlx::query_as::<_, WorkOrderTransition>(
        "SELECT * FROM work_order_transitions
         WHERE work_order_id = $1
         ORDER BY created_at ASC",
    )
    .bind(id)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "timeline", "user={} wo={} entries={}", user.user_id(), id, rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

// -----------------------------------------------------------------------------
// Soft delete (ADMIN only)
// -----------------------------------------------------------------------------
#[delete("/{id}")]
pub async fn delete_work_order(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    require_any_role(&user, &[Role::Admin])?;
    let id = path.into_inner();
    // Run soft-delete + tombstone + audit write in a single transaction so
    // either all three land or none do — strict audit guarantee (PRD §7).
    let mut tx = pool.begin().await?;
    let affected = sqlx::query(
        "UPDATE work_orders SET deleted_at = NOW(), updated_at = NOW()
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("work order not found".into()));
    }
    crate::sync::log_soft_delete_tx(&mut tx, "work_orders", id).await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::WO_DELETE,
        "work_orders",
        Some(id),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    log_warn!(MODULE, "delete", "user={} wo={} soft-deleted", user.user_id(), id);
    Ok(HttpResponse::NoContent().finish())
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Load a work order subject to the caller's visibility scope. Returns 404
/// when the caller lacks access, NOT 403 — avoids enumeration leaks.
pub async fn load_visible(
    pool: &PgPool,
    user: &AuthedUser,
    id: Uuid,
) -> Result<WorkOrder, ApiError> {
    let wo: Option<WorkOrder> =
        sqlx::query_as::<_, WorkOrder>("SELECT * FROM work_orders WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let wo = wo.ok_or_else(|| ApiError::NotFound("work order not found".into()))?;

    let visible = match user.role() {
        Role::Tech => wo.assigned_tech_id == Some(user.user_id()),
        Role::Super => {
            // Fail-closed visibility (audit AR-1 High): a SUPER MUST have a
            // branch claim, and the work order MUST belong to that branch.
            // Null-branch SUPER or branch-less work orders are hidden.
            let u_b = require_branch(user)?;
            wo.branch_id == Some(u_b)
        }
        Role::Admin => true,
    };
    if !visible {
        return Err(ApiError::NotFound("work order not found".into()));
    }
    Ok(wo)
}

// -----------------------------------------------------------------------------
// Scope wiring
// -----------------------------------------------------------------------------
pub fn scope() -> actix_web::Scope {
    web::scope("/api/work-orders")
        .service(on_call_queue)
        .service(list_work_orders)
        .service(create_work_order)
        .service(get_work_order)
        .service(delete_work_order)
        .service(transition_state)
        .service(timeline)
        .service(progress::list_progress)
        .service(progress::upsert_progress)
        .service(crate::location::routes::post_trail_point)
        .service(crate::location::routes::get_trail)
        .service(crate::location::routes::post_check_in)
}
