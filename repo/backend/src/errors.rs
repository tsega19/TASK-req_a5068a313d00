//! Unified API error type. Maps to standard HTTP status codes + JSON body
//! `{"error": "...", "code": "..."}`. Never leaks stack traces.
//!
//! Internal errors (5xx) carry a detailed message for server-side logs but
//! respond with a **generic** public message so database text, table names,
//! and query fragments don't land in client responses.

use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;

use crate::log_error;

#[derive(Debug)]
pub enum ApiError {
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    code: &'a str,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Unauthorized(m) => write!(f, "{}", m),
            ApiError::Forbidden(m) => write!(f, "{}", m),
            ApiError::NotFound(m) => write!(f, "{}", m),
            ApiError::BadRequest(m) => write!(f, "{}", m),
            ApiError::Conflict(m) => write!(f, "{}", m),
            ApiError::Internal(m) => write!(f, "{}", m),
        }
    }
}

// Required so `?` can convert `ApiError` into `anyhow::Error` (and so
// `actix_web::ResponseError`'s supertrait bound is satisfied).
impl std::error::Error for ApiError {}

impl ApiError {
    fn code(&self) -> &'static str {
        match self {
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::NotFound(_) => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Conflict(_) => "conflict",
            ApiError::Internal(_) => "internal_error",
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        // For 5xx, log the detailed message server-side and return a generic
        // public body — never expose database text, query fragments, or the
        // underlying driver message.
        let public_msg: String = match self {
            ApiError::Internal(detail) => {
                log_error!("errors", "internal", "{}", detail);
                "internal server error".to_string()
            }
            other => other.to_string(),
        };
        HttpResponse::build(self.status_code()).json(ErrorBody {
            error: &public_msg,
            code: self.code(),
        })
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ApiError::NotFound("resource not found".into()),
            // Keep the detailed message so server logs remain useful, but the
            // HTTP response replaces it with a generic string (see error_response).
            other => ApiError::Internal(format!("database error: {}", other)),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}
