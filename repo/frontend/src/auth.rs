//! Persistent auth state — token + user kept in localStorage.

use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};

use crate::types::User;

const KEY_TOKEN: &str = "fieldops_token";
const KEY_USER: &str = "fieldops_user";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct AuthState {
    pub token: Option<String>,
    pub user: Option<User>,
}

impl AuthState {
    pub fn load() -> Self {
        let token = LocalStorage::get::<String>(KEY_TOKEN).ok();
        let user = LocalStorage::get::<User>(KEY_USER).ok();
        Self { token, user }
    }

    pub fn save(token: &str, user: &User) -> Self {
        let _ = LocalStorage::set(KEY_TOKEN, token);
        let _ = LocalStorage::set(KEY_USER, user);
        Self {
            token: Some(token.to_string()),
            user: Some(user.clone()),
        }
    }

    pub fn clear() -> Self {
        LocalStorage::delete(KEY_TOKEN);
        LocalStorage::delete(KEY_USER);
        Self::default()
    }

    pub fn is_authed(&self) -> bool {
        self.token.is_some() && self.user.is_some()
    }
}
