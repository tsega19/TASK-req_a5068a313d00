//! Full coverage of the work-order state machine.
//!
//! The inline tests in `src/state_machine.rs` are spot checks; these walk the
//! complete (from, to) × role matrix defined by PRD §5 and verify that the
//! `TransitionContext::validate_required` grid rejects the exact cases PRD
//! §5 lists as required-field gates.

use fieldops_backend::auth::models::Role;
use fieldops_backend::enums::WorkOrderState as S;
use fieldops_backend::errors::ApiError;
use fieldops_backend::state_machine::{allowed_transition, TransitionContext};

fn ctx_min() -> TransitionContext {
    TransitionContext {
        notes: None,
        lat: None,
        lng: None,
        arrival_check_in_present: false,
        arrival_within_radius: false,
        departure_check_in_present: false,
        all_steps_completed: false,
    }
}

fn err_is_forbidden(e: &ApiError) -> bool {
    matches!(e, ApiError::Forbidden(_))
}
fn err_is_bad_request(e: &ApiError) -> bool {
    matches!(e, ApiError::BadRequest(_))
}

// -----------------------------------------------------------------------------
// Role × transition matrix (PRD §5)
// -----------------------------------------------------------------------------

/// Table of every `(from, to)` pair the PRD lists, with the role that MUST be
/// permitted. Any role not in the allowed set must yield Forbidden (for a
/// valid transition) or BadRequest (for an edge the PRD doesn't enumerate).
#[test]
fn transition_matrix_exhaustive() {
    // (from, to, allowed_roles)
    let cases: &[(S, S, &[Role])] = &[
        (S::Scheduled,      S::EnRoute,        &[Role::Tech]),
        (S::EnRoute,        S::OnSite,         &[Role::Tech]),
        (S::OnSite,         S::InProgress,     &[Role::Tech]),
        (S::InProgress,     S::WaitingOnParts, &[Role::Tech]),
        (S::WaitingOnParts, S::InProgress,     &[Role::Tech, Role::Super]),
        (S::InProgress,     S::Completed,      &[Role::Tech]),
        (S::Scheduled,      S::Canceled,       &[Role::Super, Role::Admin]),
        (S::EnRoute,        S::Canceled,       &[Role::Super, Role::Admin]),
        (S::OnSite,         S::Canceled,       &[Role::Super, Role::Admin]),
        (S::InProgress,     S::Canceled,       &[Role::Super, Role::Admin]),
        (S::WaitingOnParts, S::Canceled,       &[Role::Super, Role::Admin]),
    ];
    let every_role = [Role::Tech, Role::Super, Role::Admin];

    for (from, to, allowed) in cases {
        for role in every_role {
            let r = allowed_transition(*from, *to, role);
            if allowed.contains(&role) {
                assert!(
                    r.is_ok(),
                    "{:?} → {:?} should be allowed for {:?}",
                    from,
                    to,
                    role
                );
            } else {
                let e = r.expect_err(&format!(
                    "{:?} → {:?} must be rejected for {:?}",
                    from, to, role
                ));
                // Transition itself is legal; only the role is wrong → 403 Forbidden.
                assert!(
                    err_is_forbidden(&e),
                    "role-miss on a defined transition must be Forbidden, got {:?}",
                    e
                );
            }
        }
    }
}

#[test]
fn undefined_transitions_are_bad_request_for_all_roles() {
    // A few invalid pairs the PRD does not list.
    let invalid: &[(S, S)] = &[
        (S::Scheduled, S::Completed),
        (S::Scheduled, S::InProgress),
        (S::Completed, S::Scheduled),
        (S::Completed, S::EnRoute),
        (S::Canceled, S::EnRoute),
        (S::OnSite, S::Scheduled),
    ];
    for (from, to) in invalid {
        for role in [Role::Tech, Role::Super, Role::Admin] {
            let e = allowed_transition(*from, *to, role)
                .expect_err(&format!("{:?}→{:?} should be undefined", from, to));
            assert!(
                err_is_bad_request(&e),
                "undefined transitions should be BadRequest, got {:?}",
                e
            );
        }
    }
}

#[test]
fn self_transition_is_bad_request_for_every_role() {
    for s in [
        S::Scheduled,
        S::EnRoute,
        S::OnSite,
        S::InProgress,
        S::WaitingOnParts,
        S::Completed,
        S::Canceled,
    ] {
        for role in [Role::Tech, Role::Super, Role::Admin] {
            let e = allowed_transition(s, s, role).expect_err("self-transition is invalid");
            assert!(err_is_bad_request(&e));
        }
    }
}

// -----------------------------------------------------------------------------
// Required-fields grid per (from, to)
// -----------------------------------------------------------------------------

#[test]
fn enroute_requires_lat_and_lng() {
    let mut c = ctx_min();
    assert!(err_is_bad_request(
        &c.validate_required(S::Scheduled, S::EnRoute).unwrap_err()
    ));
    c.lat = Some(37.7749);
    // lng still missing
    assert!(err_is_bad_request(
        &c.validate_required(S::Scheduled, S::EnRoute).unwrap_err()
    ));
    c.lng = Some(-122.4194);
    assert!(c.validate_required(S::Scheduled, S::EnRoute).is_ok());
}

#[test]
fn onsite_requires_arrival_check_in_present_and_within_radius() {
    let mut c = ctx_min();
    // not present
    let e = c.validate_required(S::EnRoute, S::OnSite).unwrap_err();
    assert!(format!("{}", e).contains("arrival check-in"));
    // present but outside radius
    c.arrival_check_in_present = true;
    c.arrival_within_radius = false;
    let e = c.validate_required(S::EnRoute, S::OnSite).unwrap_err();
    assert!(format!("{}", e).contains("radius"));
    // both OK
    c.arrival_within_radius = true;
    assert!(c.validate_required(S::EnRoute, S::OnSite).is_ok());
}

#[test]
fn waiting_on_parts_requires_non_empty_notes() {
    let mut c = ctx_min();
    assert!(err_is_bad_request(
        &c.validate_required(S::InProgress, S::WaitingOnParts).unwrap_err()
    ));
    c.notes = Some("   ".into()); // whitespace alone must not satisfy
    assert!(c
        .validate_required(S::InProgress, S::WaitingOnParts)
        .is_err());
    c.notes = Some("awaiting compressor #4".into());
    assert!(c
        .validate_required(S::InProgress, S::WaitingOnParts)
        .is_ok());
}

#[test]
fn completed_requires_all_steps_and_departure_check_in() {
    let mut c = ctx_min();
    // both gates missing
    assert!(c.validate_required(S::InProgress, S::Completed).is_err());
    // only steps gate passing
    c.all_steps_completed = true;
    assert!(c.validate_required(S::InProgress, S::Completed).is_err());
    // only departure passing
    c.all_steps_completed = false;
    c.departure_check_in_present = true;
    assert!(c.validate_required(S::InProgress, S::Completed).is_err());
    // both pass
    c.all_steps_completed = true;
    assert!(c.validate_required(S::InProgress, S::Completed).is_ok());
}

#[test]
fn canceled_from_any_source_requires_notes() {
    for from in [
        S::Scheduled,
        S::EnRoute,
        S::OnSite,
        S::InProgress,
        S::WaitingOnParts,
    ] {
        let c = ctx_min();
        assert!(c.validate_required(from, S::Canceled).is_err());
        let c2 = TransitionContext {
            notes: Some("customer cancelled".into()),
            ..ctx_min()
        };
        assert!(c2.validate_required(from, S::Canceled).is_ok());
    }
}

#[test]
fn transitions_without_required_fields_grid_are_open() {
    // Transitions that have no required-fields gate should pass with default ctx.
    // e.g. OnSite → InProgress (auto-triggers server-side).
    let c = ctx_min();
    assert!(c.validate_required(S::OnSite, S::InProgress).is_ok());
    assert!(c.validate_required(S::WaitingOnParts, S::InProgress).is_ok());
}
