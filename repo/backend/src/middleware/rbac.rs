//! RBAC middleware skeleton.
//!
//! - `JwtAuth` middleware extracts the `Authorization: Bearer <token>` header,
//!   verifies it against `AppConfig::auth`, and inserts `Claims` into the
//!   request extensions so handlers and route guards can read them.
//! - `JwtAuth` also enforces the PRD §6 password-reset gate: once a user row
//!   has `password_reset_required = TRUE`, every authenticated path other
//!   than `/api/auth/change-password`, `/api/auth/logout`, and the health
//!   endpoints returns `403 password_reset_required` — the flag is not just
//!   documented, it's the authoritative gatekeeper.
//! - `AuthedUser` is an Actix extractor handlers use to access the claims.
//! - `require_role(role)` / `require_any_role(&[…])` produce guard-style
//!   wrappers used at the route level; this gives two enforcement layers
//!   (middleware + explicit per-route check) per PRD rule.
//!
//! Object-level authorization (BOLA/IDOR) is enforced in the handler layer
//! using `AuthedUser::role` + branch/ownership checks.

use actix_service::{Service, Transform};
use actix_web::body::EitherBody;
use actix_web::{
    dev::{ServiceRequest, ServiceResponse},
    http::header,
    Error, FromRequest, HttpMessage, HttpResponse,
};
use futures_util::future::{ready, LocalBoxFuture, Ready};
use sqlx::PgPool;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::auth::jwt::{verify, Claims};
use crate::auth::models::Role;
use crate::config::AppConfig;
use crate::errors::ApiError;

/// Routes that an authenticated user MUST be able to reach even when
/// `password_reset_required = TRUE`. Everything else is denied until the user
/// rotates the password. Keep the list minimal — PRD §6.
///
/// `/api/me` (profile lookup) is exempt so the client can render "who am I"
/// on the forced-change-password screen without a second authenticated round
/// trip. The sub-paths under `/api/me/*` (privacy toggle, home-address
/// write/read) remain *privileged* and stay behind the gate.
fn is_reset_exempt_path(path: &str) -> bool {
    matches!(
        path,
        "/api/auth/change-password"
            | "/api/auth/logout"
            | "/health"
            | "/api/health"
            | "/api/me"
    )
}

// -----------------------------------------------------------------------------
// JwtAuth middleware
// -----------------------------------------------------------------------------

pub struct JwtAuth;

impl<S, B> Transform<S, ServiceRequest> for JwtAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Transform = JwtAuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(JwtAuthMiddleware {
            service: Rc::new(service),
        }))
    }
}

pub struct JwtAuthMiddleware<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for JwtAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let srv = self.service.clone();
        Box::pin(async move {
            // Only the unauthenticated auth endpoints are public; health too.
            // /api/auth/change-password MUST go through JWT auth.
            // Logout requires a valid bearer — stateless JWT can't be revoked
            // server-side here, but we still force callers to prove identity
            // before the endpoint acknowledges, per security review.
            let path = req.path().to_string();
            let public = path == "/api/auth/login"
                || path == "/health"
                || path == "/api/health";

            if !public {
                let header_val = req
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|h| h.to_str().ok())
                    .map(|s| s.to_string());

                let token = match header_val {
                    Some(ref v) if v.starts_with("Bearer ") => v[7..].to_string(),
                    _ => {
                        let resp = HttpResponse::Unauthorized()
                            .json(serde_json::json!({"error":"missing bearer token","code":"unauthorized"}));
                        return Ok(req.into_response(resp.map_into_right_body()));
                    }
                };

                let cfg = match req.app_data::<actix_web::web::Data<AppConfig>>() {
                    Some(c) => c.clone(),
                    None => {
                        let resp = HttpResponse::InternalServerError()
                            .json(serde_json::json!({"error":"config missing","code":"internal_error"}));
                        return Ok(req.into_response(resp.map_into_right_body()));
                    }
                };

                let claims = match verify(&token, &cfg.auth) {
                    Ok(claims) => claims,
                    Err(_) => {
                        let resp = HttpResponse::Unauthorized()
                            .json(serde_json::json!({"error":"invalid token","code":"unauthorized"}));
                        return Ok(req.into_response(resp.map_into_right_body()));
                    }
                };

                // PRD §6 security gate: a user flagged `password_reset_required`
                // must rotate before executing any privileged action. Enforced
                // server-side — not just advertised in the login response. The
                // gate fails closed: a missing pool or DB error denies the call.
                if !is_reset_exempt_path(&path) {
                    let pool = match req.app_data::<actix_web::web::Data<PgPool>>() {
                        Some(p) => p.clone(),
                        None => {
                            let resp = HttpResponse::InternalServerError()
                                .json(serde_json::json!({"error":"pool missing","code":"internal_error"}));
                            return Ok(req.into_response(resp.map_into_right_body()));
                        }
                    };
                    let reset_required: Result<Option<bool>, _> = sqlx::query_scalar(
                        "SELECT password_reset_required FROM users
                         WHERE id = $1 AND deleted_at IS NULL",
                    )
                    .bind(claims.sub)
                    .fetch_optional(pool.get_ref())
                    .await;
                    match reset_required {
                        Ok(Some(true)) => {
                            let resp = HttpResponse::Forbidden().json(serde_json::json!({
                                "error": "password reset required before privileged actions",
                                "code": "password_reset_required"
                            }));
                            return Ok(req.into_response(resp.map_into_right_body()));
                        }
                        Ok(None) => {
                            // User missing or soft-deleted — treat as unauthenticated.
                            let resp = HttpResponse::Unauthorized().json(serde_json::json!({
                                "error": "user not found",
                                "code": "unauthorized"
                            }));
                            return Ok(req.into_response(resp.map_into_right_body()));
                        }
                        Ok(Some(false)) => {}
                        Err(_) => {
                            let resp = HttpResponse::InternalServerError().json(
                                serde_json::json!({"error":"auth gate db error","code":"internal_error"}),
                            );
                            return Ok(req.into_response(resp.map_into_right_body()));
                        }
                    }
                }

                req.extensions_mut().insert(claims);
            }

            let res = srv.call(req).await?;
            Ok(res.map_into_left_body())
        })
    }
}

// -----------------------------------------------------------------------------
// AuthedUser extractor
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AuthedUser(pub Claims);

impl AuthedUser {
    pub fn role(&self) -> Role {
        self.0.role
    }
    pub fn user_id(&self) -> uuid::Uuid {
        self.0.sub
    }
    pub fn branch_id(&self) -> Option<uuid::Uuid> {
        self.0.branch_id
    }
}

impl FromRequest for AuthedUser {
    type Error = ApiError;
    type Future = Ready<Result<Self, ApiError>>;

    fn from_request(req: &actix_web::HttpRequest, _pl: &mut actix_web::dev::Payload) -> Self::Future {
        let ext = req.extensions();
        match ext.get::<Claims>().cloned() {
            Some(claims) => ready(Ok(AuthedUser(claims))),
            None => ready(Err(ApiError::Unauthorized("not authenticated".into()))),
        }
    }
}

// -----------------------------------------------------------------------------
// Route-level role guards
// -----------------------------------------------------------------------------

/// Call from inside a handler to enforce the route-level role check required
/// by the PRD ("enforce at route AND middleware level").
pub fn require_role(user: &AuthedUser, required: Role) -> Result<(), ApiError> {
    if user.role() == required {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!(
            "role {} required, have {}",
            required,
            user.role()
        )))
    }
}

pub fn require_any_role(user: &AuthedUser, allowed: &[Role]) -> Result<(), ApiError> {
    if allowed.iter().any(|r| *r == user.role()) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!(
            "one of {:?} required, have {}",
            allowed,
            user.role()
        )))
    }
}

/// Return the caller's branch_id for branch-scoped reads, or a 403 when the
/// principal has no branch assignment. Scope SQL MUST fail closed for
/// SUPER/TECH — a null branch claim must never widen to "see everything"
/// (audit AR-1 High). ADMIN code paths should not call this; they use a
/// global scope by design.
pub fn require_branch(user: &AuthedUser) -> Result<uuid::Uuid, ApiError> {
    user.branch_id().ok_or_else(|| {
        ApiError::Forbidden(
            "role requires a branch assignment; contact an administrator".into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwt::Claims;
    use chrono::Utc;

    fn user(role: Role) -> AuthedUser {
        AuthedUser(Claims {
            sub: uuid::Uuid::new_v4(),
            username: "u".into(),
            role,
            branch_id: None,
            iat: Utc::now().timestamp(),
            exp: Utc::now().timestamp() + 3600,
            iss: "fieldops-test".into(),
            aud: "fieldops-test".into(),
        })
    }

    #[test]
    fn require_role_matches() {
        assert!(require_role(&user(Role::Admin), Role::Admin).is_ok());
        assert!(require_role(&user(Role::Tech), Role::Admin).is_err());
    }

    #[test]
    fn require_any_role_matches() {
        assert!(require_any_role(&user(Role::Super), &[Role::Super, Role::Admin]).is_ok());
        assert!(require_any_role(&user(Role::Tech), &[Role::Super, Role::Admin]).is_err());
    }
}
