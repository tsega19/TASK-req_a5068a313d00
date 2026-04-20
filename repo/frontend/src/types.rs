//! Shared API/data types. Kept thin — only fields the UI reads.
//!
//! The `#[cfg(test)]` module at the bottom carries executable
//! `wasm_bindgen_test` cases for pure logic (role/state/status labelling,
//! state-machine helpers). Run with
//! `wasm-pack test --headless --chrome frontend` from the repo root.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "TECH")]
    Tech,
    #[serde(rename = "SUPER")]
    Super,
    #[serde(rename = "ADMIN")]
    Admin,
}

impl Role {
    pub fn label(&self) -> &'static str {
        match self {
            Role::Tech => "Technician",
            Role::Super => "Supervisor",
            Role::Admin => "Administrator",
        }
    }
    pub fn short(&self) -> &'static str {
        match self {
            Role::Tech => "TECH",
            Role::Super => "SUPER",
            Role::Admin => "ADMIN",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub role: Role,
    pub branch_id: Option<Uuid>,
    pub full_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: Uuid,
    pub username: String,
    pub role: Role,
    pub branch_id: Option<Uuid>,
    pub full_name: Option<String>,
    pub privacy_mode: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Priority {
    #[serde(rename = "LOW")]
    Low,
    #[serde(rename = "NORMAL")]
    Normal,
    #[serde(rename = "HIGH")]
    High,
    #[serde(rename = "CRITICAL")]
    Critical,
}

impl Priority {
    pub fn label(&self) -> &'static str {
        match self {
            Priority::Low => "LOW",
            Priority::Normal => "NORMAL",
            Priority::High => "HIGH",
            Priority::Critical => "CRITICAL",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkOrderState {
    Scheduled,
    EnRoute,
    OnSite,
    InProgress,
    WaitingOnParts,
    Completed,
    Canceled,
}

impl WorkOrderState {
    pub fn label(&self) -> &'static str {
        match self {
            WorkOrderState::Scheduled => "Scheduled",
            WorkOrderState::EnRoute => "En Route",
            WorkOrderState::OnSite => "On Site",
            WorkOrderState::InProgress => "In Progress",
            WorkOrderState::WaitingOnParts => "Waiting on Parts",
            WorkOrderState::Completed => "Completed",
            WorkOrderState::Canceled => "Canceled",
        }
    }
    pub fn css_key(&self) -> &'static str {
        match self {
            WorkOrderState::Scheduled => "Scheduled",
            WorkOrderState::EnRoute => "EnRoute",
            WorkOrderState::OnSite => "OnSite",
            WorkOrderState::InProgress => "InProgress",
            WorkOrderState::WaitingOnParts => "WaitingOnParts",
            WorkOrderState::Completed => "Completed",
            WorkOrderState::Canceled => "Canceled",
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, WorkOrderState::Completed | WorkOrderState::Canceled)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkOrder {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub priority: Priority,
    pub state: WorkOrderState,
    pub assigned_tech_id: Option<Uuid>,
    pub branch_id: Option<Uuid>,
    pub sla_deadline: Option<chrono::DateTime<chrono::Utc>>,
    pub recipe_id: Option<Uuid>,
    pub location_address_norm: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
    pub etag: Option<String>,
    pub version_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paginated<T> {
    pub data: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataEnvelope<T> {
    pub data: Vec<T>,
    pub total: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeStep {
    pub id: Uuid,
    pub recipe_id: Uuid,
    pub step_order: i32,
    pub title: String,
    pub instructions: Option<String>,
    pub is_pauseable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipCard {
    pub id: Uuid,
    pub step_id: Uuid,
    pub title: String,
    pub content: String,
    pub is_pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StepProgressStatus {
    Pending,
    InProgress,
    Paused,
    Completed,
}

impl StepProgressStatus {
    pub fn label(&self) -> &'static str {
        match self {
            StepProgressStatus::Pending => "Pending",
            StepProgressStatus::InProgress => "In Progress",
            StepProgressStatus::Paused => "Paused",
            StepProgressStatus::Completed => "Completed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepProgress {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub step_id: Uuid,
    pub status: StepProgressStatus,
    pub notes: Option<String>,
    pub version: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailPoint {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub user_id: Uuid,
    pub lat: f64,
    pub lng: f64,
    pub precision_reduced: bool,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CheckInType {
    #[serde(rename = "ARRIVAL")]
    Arrival,
    #[serde(rename = "DEPARTURE")]
    Departure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NotificationTemplate {
    #[serde(rename = "SIGNUP_SUCCESS")]
    SignupSuccess,
    #[serde(rename = "SCHEDULE_CHANGE")]
    ScheduleChange,
    #[serde(rename = "CANCELLATION")]
    Cancellation,
    #[serde(rename = "REVIEW_RESULT")]
    ReviewResult,
}

impl NotificationTemplate {
    pub fn label(&self) -> &'static str {
        match self {
            NotificationTemplate::SignupSuccess => "Signup Success",
            NotificationTemplate::ScheduleChange => "Schedule Change",
            NotificationTemplate::Cancellation => "Cancellation",
            NotificationTemplate::ReviewResult => "Review Result",
        }
    }
    pub const ALL: [NotificationTemplate; 4] = [
        NotificationTemplate::SignupSuccess,
        NotificationTemplate::ScheduleChange,
        NotificationTemplate::Cancellation,
        NotificationTemplate::ReviewResult,
    ];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub template_type: NotificationTemplate,
    pub payload: serde_json::Value,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_unsubscribed: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LearningRow {
    pub user_id: Uuid,
    pub username: String,
    pub role: Role,
    pub branch_id: Option<Uuid>,
    pub quiz_avg: Option<f64>,
    pub time_spent_total: Option<i64>,
    pub completion_count: Option<i64>,
    pub review_total: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub id: Uuid,
    pub name: String,
    pub address: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub service_radius_miles: i32,
}

// ---------------------------------------------------------------------------
// Pure-logic unit tests — executable via `wasm-pack test` (audit Low #6).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn role_short_and_label_are_stable() {
        assert_eq!(Role::Tech.short(), "TECH");
        assert_eq!(Role::Super.short(), "SUPER");
        assert_eq!(Role::Admin.short(), "ADMIN");
        assert_eq!(Role::Tech.label(), "Technician");
        assert_eq!(Role::Super.label(), "Supervisor");
        assert_eq!(Role::Admin.label(), "Administrator");
    }

    #[wasm_bindgen_test]
    fn role_serde_uses_wire_tags() {
        // Server shares these tags — drifting them silently would shatter the
        // analytics role filter and the admin role selector.
        let tech = serde_json::to_string(&Role::Tech).unwrap();
        assert_eq!(tech, "\"TECH\"");
        let parsed: Role = serde_json::from_str("\"ADMIN\"").unwrap();
        assert_eq!(parsed, Role::Admin);
    }

    #[wasm_bindgen_test]
    fn work_order_state_terminal_matches_ui_gating() {
        // The transition panel hides itself for terminal states; the server
        // also rejects outbound transitions from them. This mapping must stay
        // aligned with both.
        assert!(WorkOrderState::Completed.is_terminal());
        assert!(WorkOrderState::Canceled.is_terminal());
        for s in [
            WorkOrderState::Scheduled,
            WorkOrderState::EnRoute,
            WorkOrderState::OnSite,
            WorkOrderState::InProgress,
            WorkOrderState::WaitingOnParts,
        ] {
            assert!(!s.is_terminal(), "{:?} must not be terminal", s);
        }
    }

    #[wasm_bindgen_test]
    fn work_order_state_css_key_matches_server_variants() {
        // CSS class names must match the backend's enum variant names exactly
        // — the stylesheet keys badge colors off them.
        assert_eq!(WorkOrderState::EnRoute.css_key(), "EnRoute");
        assert_eq!(WorkOrderState::WaitingOnParts.css_key(), "WaitingOnParts");
        assert_eq!(WorkOrderState::OnSite.css_key(), "OnSite");
    }

    #[wasm_bindgen_test]
    fn step_progress_status_labels_are_human_readable() {
        assert_eq!(StepProgressStatus::Pending.label(), "Pending");
        assert_eq!(StepProgressStatus::InProgress.label(), "In Progress");
        assert_eq!(StepProgressStatus::Paused.label(), "Paused");
        assert_eq!(StepProgressStatus::Completed.label(), "Completed");
    }

    #[wasm_bindgen_test]
    fn priority_label_matches_server_enum() {
        // Server sends priorities as UPPERCASE tags. The label helper exposes
        // them verbatim so dashboards show the same string as the DB.
        assert_eq!(Priority::Low.label(), "LOW");
        assert_eq!(Priority::Critical.label(), "CRITICAL");
        let json = serde_json::to_string(&Priority::High).unwrap();
        assert_eq!(json, "\"HIGH\"");
    }
}
