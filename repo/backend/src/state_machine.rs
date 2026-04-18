//! Work-order state machine (PRD §5).
//!
//! Validation is split in two: `allowed_transition` handles role + edge
//! legality; `TransitionContext::validate_required` handles the per-transition
//! required fields (GPS, notes, arrival check-in, step completion gate,
//! departure check-in). The route handler passes already-loaded context rather
//! than reaching back into the DB from this module.

use crate::auth::models::Role;
use crate::enums::WorkOrderState;
use crate::errors::ApiError;

pub struct TransitionContext {
    pub notes: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub arrival_check_in_present: bool,
    pub arrival_within_radius: bool,
    pub departure_check_in_present: bool,
    pub all_steps_completed: bool,
}

pub fn allowed_transition(
    from: WorkOrderState,
    to: WorkOrderState,
    role: Role,
) -> Result<(), ApiError> {
    use Role::*;
    use WorkOrderState::*;

    if from == to {
        return Err(ApiError::BadRequest("already in requested state".into()));
    }

    let role_ok = match (from, to) {
        (Scheduled, EnRoute) => role == Tech,
        (EnRoute, OnSite) => role == Tech,
        (OnSite, InProgress) => role == Tech,
        (InProgress, WaitingOnParts) => role == Tech,
        (WaitingOnParts, InProgress) => role == Tech || role == Super,
        (InProgress, Completed) => role == Tech,
        (Scheduled | EnRoute | OnSite | InProgress | WaitingOnParts, Canceled) => {
            role == Super || role == Admin
        }
        _ => {
            return Err(ApiError::BadRequest(format!(
                "transition {:?} → {:?} not allowed",
                from, to
            )));
        }
    };
    if !role_ok {
        return Err(ApiError::Forbidden(format!(
            "role {} cannot perform {:?} → {:?}",
            role, from, to
        )));
    }
    Ok(())
}

impl TransitionContext {
    pub fn validate_required(
        &self,
        from: WorkOrderState,
        to: WorkOrderState,
    ) -> Result<(), ApiError> {
        use WorkOrderState::*;
        match (from, to) {
            (Scheduled, EnRoute) => {
                if self.lat.is_none() || self.lng.is_none() {
                    return Err(ApiError::BadRequest("EnRoute transition requires lat/lng".into()));
                }
            }
            (EnRoute, OnSite) => {
                if !self.arrival_check_in_present {
                    return Err(ApiError::BadRequest(
                        "OnSite transition requires an arrival check-in".into(),
                    ));
                }
                if !self.arrival_within_radius {
                    return Err(ApiError::BadRequest(
                        "arrival check-in is outside the branch service radius".into(),
                    ));
                }
            }
            (InProgress, WaitingOnParts) => {
                self.require_notes("WaitingOnParts")?;
            }
            (InProgress, Completed) => {
                if !self.all_steps_completed {
                    return Err(ApiError::BadRequest(
                        "Completed blocked: not all steps are Completed".into(),
                    ));
                }
                if !self.departure_check_in_present {
                    return Err(ApiError::BadRequest(
                        "Completed transition requires a departure check-in".into(),
                    ));
                }
            }
            (_, Canceled) => {
                self.require_notes("Canceled")?;
            }
            _ => {}
        }
        Ok(())
    }

    fn require_notes(&self, label: &str) -> Result<(), ApiError> {
        match &self.notes {
            Some(n) if !n.trim().is_empty() => Ok(()),
            _ => Err(ApiError::BadRequest(format!("{label} transition requires notes"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TransitionContext {
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

    #[test]
    fn tech_can_start_en_route() {
        assert!(allowed_transition(WorkOrderState::Scheduled, WorkOrderState::EnRoute, Role::Tech).is_ok());
    }

    #[test]
    fn super_cannot_start_en_route() {
        assert!(allowed_transition(WorkOrderState::Scheduled, WorkOrderState::EnRoute, Role::Super).is_err());
    }

    #[test]
    fn only_super_or_admin_cancels() {
        assert!(allowed_transition(WorkOrderState::Scheduled, WorkOrderState::Canceled, Role::Tech).is_err());
        assert!(allowed_transition(WorkOrderState::Scheduled, WorkOrderState::Canceled, Role::Super).is_ok());
        assert!(allowed_transition(WorkOrderState::Scheduled, WorkOrderState::Canceled, Role::Admin).is_ok());
    }

    #[test]
    fn onsite_requires_arrival_checkin() {
        let c = ctx();
        let err = c.validate_required(WorkOrderState::EnRoute, WorkOrderState::OnSite).unwrap_err();
        assert!(format!("{}", err).contains("arrival check-in"));
    }

    #[test]
    fn completed_requires_all_steps_and_departure() {
        let mut c = ctx();
        c.all_steps_completed = false;
        assert!(c.validate_required(WorkOrderState::InProgress, WorkOrderState::Completed).is_err());
        c.all_steps_completed = true;
        c.departure_check_in_present = false;
        assert!(c.validate_required(WorkOrderState::InProgress, WorkOrderState::Completed).is_err());
        c.departure_check_in_present = true;
        assert!(c.validate_required(WorkOrderState::InProgress, WorkOrderState::Completed).is_ok());
    }

    #[test]
    fn canceled_requires_notes() {
        let c = ctx();
        assert!(c.validate_required(WorkOrderState::Scheduled, WorkOrderState::Canceled).is_err());
        let c2 = TransitionContext { notes: Some("broken part".into()), ..ctx() };
        assert!(c2.validate_required(WorkOrderState::Scheduled, WorkOrderState::Canceled).is_ok());
    }

    #[test]
    fn no_transition_to_self() {
        assert!(allowed_transition(WorkOrderState::OnSite, WorkOrderState::OnSite, Role::Tech).is_err());
    }
}
