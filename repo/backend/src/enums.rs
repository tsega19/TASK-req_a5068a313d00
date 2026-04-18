//! Shared PostgreSQL enum types mapped to Rust enums. Kept in one file so
//! every module can reference them without circular imports.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "work_order_priority", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "work_order_state")]
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
    pub fn is_terminal(self) -> bool {
        matches!(self, WorkOrderState::Completed | WorkOrderState::Canceled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "timer_alert_type", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum TimerAlertType {
    Audible,
    Visual,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "step_progress_status")]
pub enum StepProgressStatus {
    Pending,
    InProgress,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "check_in_type", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum CheckInType {
    Arrival,
    Departure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "notification_template_type", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationTemplate {
    SignupSuccess,
    ScheduleChange,
    Cancellation,
    ReviewResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "sync_operation", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum SyncOperation {
    Insert,
    Update,
    Delete,
}
