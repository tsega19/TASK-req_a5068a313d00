use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{Priority, WorkOrderState};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WorkOrder {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub priority: Priority,
    pub state: WorkOrderState,
    pub assigned_tech_id: Option<Uuid>,
    pub branch_id: Option<Uuid>,
    pub sla_deadline: Option<DateTime<Utc>>,
    pub recipe_id: Option<Uuid>,
    pub location_address_norm: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
    pub etag: Option<String>,
    pub version_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WorkOrderTransition {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub from_state: Option<String>,
    pub to_state: String,
    pub triggered_by: Option<Uuid>,
    pub required_fields: serde_json::Value,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkOrder {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub assigned_tech_id: Option<Uuid>,
    pub branch_id: Option<Uuid>,
    pub sla_deadline: Option<DateTime<Utc>>,
    pub recipe_id: Option<Uuid>,
    pub location_address_norm: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct StateTransitionRequest {
    pub to_state: WorkOrderState,
    pub notes: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
}
