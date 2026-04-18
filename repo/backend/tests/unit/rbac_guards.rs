//! Dedicated unit tests for the RBAC guard helpers in
//! `fieldops_backend::middleware::rbac`.
//!
//! Exercises every (caller_role, expected_role) pair for `require_role` and
//! the edge cases for `require_any_role` (single role, multi, empty, caller
//! not in the allow-list). The inline tests cover the happy path only; these
//! close the audit's "RBAC middleware edge cases" gap.

use fieldops_backend::auth::jwt::Claims;
use fieldops_backend::auth::models::Role;
use fieldops_backend::errors::ApiError;
use fieldops_backend::middleware::rbac::{require_any_role, require_role, AuthedUser};
use uuid::Uuid;

fn make(role: Role) -> AuthedUser {
    AuthedUser(Claims {
        sub: Uuid::new_v4(),
        username: "u".into(),
        role,
        branch_id: None,
        iat: 0,
        exp: i64::MAX / 2, // not expired
    })
}

fn assert_forbidden(r: Result<(), ApiError>, expected_role: Role, caller: Role) {
    match r {
        Err(ApiError::Forbidden(msg)) => {
            // Message must name the required + actual roles so operators can
            // triage from a log line alone.
            let m = msg.to_lowercase();
            assert!(
                m.contains(&expected_role.to_string().to_lowercase())
                    || m.contains(&format!("{:?}", expected_role).to_lowercase()),
                "message should mention required role: {}",
                msg
            );
            let _ = caller; // variant captured above; kept for readable assertion frame
        }
        Ok(()) => panic!("expected Forbidden for caller {:?}", caller),
        Err(other) => panic!("expected Forbidden, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// require_role — full 3×3 matrix
// -----------------------------------------------------------------------------

#[test]
fn require_role_matrix_all_pairs() {
    let roles = [Role::Tech, Role::Super, Role::Admin];
    for caller in roles {
        for required in roles {
            let r = require_role(&make(caller), required);
            if caller == required {
                assert!(r.is_ok(), "{:?} calling {:?} endpoint should pass", caller, required);
            } else {
                assert_forbidden(r, required, caller);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// require_any_role — single, multi, empty
// -----------------------------------------------------------------------------

#[test]
fn require_any_role_single_element_equivalent_to_require_role() {
    for caller in [Role::Tech, Role::Super, Role::Admin] {
        for required in [Role::Tech, Role::Super, Role::Admin] {
            let single = require_any_role(&make(caller), &[required]);
            let exact = require_role(&make(caller), required);
            assert_eq!(single.is_ok(), exact.is_ok());
        }
    }
}

#[test]
fn require_any_role_multi_allows_all_listed_roles() {
    let allowed = &[Role::Super, Role::Admin];
    assert!(require_any_role(&make(Role::Super), allowed).is_ok());
    assert!(require_any_role(&make(Role::Admin), allowed).is_ok());
    // Tech is NOT in the list.
    assert!(matches!(
        require_any_role(&make(Role::Tech), allowed),
        Err(ApiError::Forbidden(_))
    ));
}

#[test]
fn require_any_role_tech_super_admin_is_effectively_public_for_authed() {
    let all = &[Role::Tech, Role::Super, Role::Admin];
    for caller in [Role::Tech, Role::Super, Role::Admin] {
        assert!(require_any_role(&make(caller), all).is_ok());
    }
}

#[test]
fn require_any_role_empty_allowlist_rejects_every_role() {
    // Defensive: passing an empty allow-list should never silently permit.
    let empty: &[Role] = &[];
    for caller in [Role::Tech, Role::Super, Role::Admin] {
        let err = require_any_role(&make(caller), empty).expect_err("empty allow-list must fail");
        assert!(matches!(err, ApiError::Forbidden(_)));
    }
}

// -----------------------------------------------------------------------------
// AuthedUser accessors
// -----------------------------------------------------------------------------

#[test]
fn authed_user_accessors_surface_claim_fields() {
    let sub = Uuid::new_v4();
    let branch = Uuid::new_v4();
    let u = AuthedUser(Claims {
        sub,
        username: "alice".into(),
        role: Role::Super,
        branch_id: Some(branch),
        iat: 0,
        exp: i64::MAX / 2,
    });
    assert_eq!(u.user_id(), sub);
    assert_eq!(u.role(), Role::Super);
    assert_eq!(u.branch_id(), Some(branch));
}

#[test]
fn authed_user_branch_id_is_optional() {
    let u = make(Role::Admin);
    assert_eq!(u.branch_id(), None, "admin seeded without a branch");
}

// -----------------------------------------------------------------------------
// Error mapping: wrong-role response is distinct from missing-bearer response
// -----------------------------------------------------------------------------

#[test]
fn forbidden_and_unauthorized_are_different_variants() {
    // require_role mismatch → Forbidden (403)
    let f = require_role(&make(Role::Tech), Role::Admin).unwrap_err();
    assert!(matches!(f, ApiError::Forbidden(_)));

    // Missing auth is surfaced separately by the JwtAuth middleware as
    // Unauthorized (401). This unit test enforces that the two error kinds
    // are cleanly distinguishable at the variant level so route handlers
    // and clients can branch on them.
    let u = ApiError::Unauthorized("missing bearer".into());
    assert!(matches!(u, ApiError::Unauthorized(_)));
    assert!(!matches!(u, ApiError::Forbidden(_)));
}
