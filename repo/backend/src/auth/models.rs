use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "UPPERCASE")]
pub enum Role {
    #[serde(rename = "TECH")]
    Tech,
    #[serde(rename = "SUPER")]
    Super,
    #[serde(rename = "ADMIN")]
    Admin,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Tech => "TECH",
            Role::Super => "SUPER",
            Role::Admin => "ADMIN",
        })
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "TECH" => Ok(Role::Tech),
            "SUPER" => Ok(Role::Super),
            "ADMIN" => Ok(Role::Admin),
            other => Err(format!("unknown role '{}'", other)),
        }
    }
}

/// Row type for `users` — small struct used by auth and admin paths.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UserRow {
    pub id: Uuid,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: Role,
    pub branch_id: Option<Uuid>,
    pub full_name: Option<String>,
    pub privacy_mode: bool,
    #[serde(default)]
    pub password_reset_required: bool,
}
