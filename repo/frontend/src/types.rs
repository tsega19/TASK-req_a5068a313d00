//! Shared API/data types. Kept thin — only fields the UI reads.

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
