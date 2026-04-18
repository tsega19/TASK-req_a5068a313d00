//! Centralized structured logger. Format: `[module][sub-module] message`.
//! Automatically redacts `password`, `token`, `secret`, `authorization`
//! substrings from any emitted message.

use once_cell::sync::OnceCell;
use regex::Regex;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

static REDACTOR: OnceCell<Vec<Regex>> = OnceCell::new();

pub fn init(level: &str, format: &str) {
    let _ = REDACTOR.set(vec![
        Regex::new(r#"(?i)("?(?:password|passwd|pwd)"?\s*[:=]\s*")[^"]*(")"#).unwrap(),
        Regex::new(r#"(?i)("?(?:token|jwt|authorization|bearer|secret|api[_-]?key)"?\s*[:=]\s*")[^"]*(")"#).unwrap(),
        Regex::new(r#"(?i)(Bearer\s+)[A-Za-z0-9\._\-]+"#).unwrap(),
    ]);

    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));

    if format.eq_ignore_ascii_case("json") {
        let fmt_layer = fmt::layer().json().with_target(true).with_current_span(false);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init();
    } else {
        let fmt_layer = fmt::layer().with_target(true);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init();
    }
}

/// Redact sensitive substrings from a user-supplied message. Applied
/// automatically by the `log!` helper below.
pub fn redact(input: &str) -> String {
    let Some(patterns) = REDACTOR.get() else {
        return input.to_string();
    };
    let mut out = input.to_string();
    for re in patterns {
        out = re.replace_all(&out, "$1<redacted>$2").to_string();
    }
    out
}

/// Format a tag pair as `[module][sub-module]`.
pub fn tag(module: &str, sub: &str) -> String {
    format!("[{}][{}]", module, sub)
}

/// Structured log helpers. Prefer these over bare `tracing::` macros so that
/// the `[module][sub-module]` prefix and redaction are applied uniformly.
#[macro_export]
macro_rules! log_info {
    ($module:expr, $sub:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let redacted = $crate::logging::redact(&msg);
        tracing::info!("{} {}", $crate::logging::tag($module, $sub), redacted);
    }};
}

#[macro_export]
macro_rules! log_warn {
    ($module:expr, $sub:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let redacted = $crate::logging::redact(&msg);
        tracing::warn!("{} {}", $crate::logging::tag($module, $sub), redacted);
    }};
}

#[macro_export]
macro_rules! log_error {
    ($module:expr, $sub:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let redacted = $crate::logging::redact(&msg);
        tracing::error!("{} {}", $crate::logging::tag($module, $sub), redacted);
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($module:expr, $sub:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let redacted = $crate::logging::redact(&msg);
        tracing::debug!("{} {}", $crate::logging::tag($module, $sub), redacted);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_strips_password_field() {
        init("info", "structured");
        let out = redact(r#"login attempt {"username":"alice","password":"hunter2"}"#);
        assert!(!out.contains("hunter2"));
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn redact_strips_bearer_token() {
        init("info", "structured");
        let out = redact("Authorization: Bearer eyJhbGciOi.foo.bar");
        assert!(!out.contains("eyJhbGciOi.foo.bar"));
    }

    #[test]
    fn tag_format_is_bracketed() {
        assert_eq!(tag("auth", "login"), "[auth][login]");
    }
}
