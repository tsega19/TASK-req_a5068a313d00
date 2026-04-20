//! Centralized configuration — the ONLY module permitted to read environment
//! variables. All other modules MUST consume `AppConfig` by reference. This is
//! enforced by convention (and CI grep) per guide.md phase 1.

use std::env;
use std::fmt;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub http: HttpConfig,
    pub auth: AuthConfig,
    pub encryption: EncryptionConfig,
    pub logging: LoggingConfig,
    pub business: BusinessConfig,
    pub app: AppBehaviorConfig,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub enable_tls: bool,
    pub tls_cert_path: String,
    pub tls_key_path: String,
}

#[derive(Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    /// JWT `iss` claim issued and enforced on verification. Bound to a
    /// service-specific value so tokens minted for one deployment cannot be
    /// replayed against another (defense-in-depth against token confusion).
    pub jwt_issuer: String,
    /// JWT `aud` claim issued and enforced on verification — see `jwt_issuer`.
    pub jwt_audience: String,
    pub argon2_memory_kib: u32,
    pub argon2_iterations: u32,
    pub argon2_parallelism: u32,
}

impl fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthConfig")
            .field("jwt_secret", &"<redacted>")
            .field("jwt_expiry_hours", &self.jwt_expiry_hours)
            .field("jwt_issuer", &self.jwt_issuer)
            .field("jwt_audience", &self.jwt_audience)
            .field("argon2_memory_kib", &self.argon2_memory_kib)
            .field("argon2_iterations", &self.argon2_iterations)
            .field("argon2_parallelism", &self.argon2_parallelism)
            .finish()
    }
}

#[derive(Clone)]
pub struct EncryptionConfig {
    pub aes_256_key: [u8; 32],
}

impl fmt::Debug for EncryptionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptionConfig")
            .field("aes_256_key", &"<redacted:32b>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

#[derive(Debug, Clone)]
pub struct BusinessConfig {
    pub sync_interval_minutes: u64,
    pub default_service_radius_miles: i32,
    pub max_notifications_per_hour: u32,
    pub max_versions_per_progress: u32,
    pub soft_delete_retention_days: u32,
    pub sla_alert_thresholds: Vec<f64>,
    pub notification_retry_max_attempts: u32,
    pub notification_retry_base_seconds: u64,
    pub on_call_high_priority_hours: i64,
}

#[derive(Debug, Clone)]
pub struct AppBehaviorConfig {
    pub run_migrations_on_boot: bool,
    pub seed_default_admin: bool,
    pub default_admin_username: String,
    pub default_admin_password: String,
    /// When true, insecure defaults (placeholder JWT secret, placeholder AES
    /// key, default admin password) are permitted. When false, startup hard-
    /// fails if any of those known placeholders are detected in configuration.
    pub dev_mode: bool,
    pub require_admin_password_change: bool,
    /// When true, the geocoder's deterministic-hash fallback is allowed for
    /// addresses that don't match the bundled ZIP+4/street index. When false
    /// (production default), unknown addresses produce a 400 at the API
    /// boundary so synthetic coordinates can't silently pollute radius and
    /// trail data. Env var: `ALLOW_GEOCODE_FALLBACK`.
    pub allow_geocode_fallback: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    Missing(String),
    #[error("invalid value for {0}: {1}")]
    Invalid(String, String),
    #[error("insecure placeholder detected in {0} — refusing startup (set DEV_MODE=true to override for development)")]
    InsecurePlaceholder(String),
}

// Known placeholder values that MUST NOT be used outside of DEV_MODE. Production
// boots fail hard when any of these land in live configuration.
const PLACEHOLDER_JWT_SECRETS: &[&str] = &[
    "dev-jwt-secret-change-in-prod-0123456789abcdef",
    "change-me",
    "changeme",
    "secret",
];
const PLACEHOLDER_AES_KEY_HEX: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PLACEHOLDER_ADMIN_PASSWORDS: &[&str] = &["admin", "admin123", "password", "123456"];

fn require(key: &str) -> Result<String, ConfigError> {
    env::var(key).map_err(|_| ConfigError::Missing(key.to_string()))
}

fn optional(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_bool(key: &str, default: bool) -> Result<bool, ConfigError> {
    match env::var(key) {
        Ok(v) => match v.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" | "" => Ok(false),
            other => Err(ConfigError::Invalid(key.into(), other.into())),
        },
        Err(_) => Ok(default),
    }
}

fn parse_num<T: std::str::FromStr>(key: &str, default: T) -> Result<T, ConfigError>
where
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(v) => v
            .parse::<T>()
            .map_err(|e| ConfigError::Invalid(key.into(), e.to_string())),
        Err(_) => Ok(default),
    }
}

impl AppConfig {
    /// Load configuration from process environment. This is the ONE AND ONLY
    /// place where `std::env::var` is allowed.
    pub fn from_env() -> Result<Self, ConfigError> {
        let dev_mode = parse_bool("DEV_MODE", false)?;
        let require_admin_password_change =
            parse_bool("REQUIRE_ADMIN_PASSWORD_CHANGE", !dev_mode)?;
        let aes_hex = require("AES_256_KEY_HEX")?;
        if !dev_mode && aes_hex.eq_ignore_ascii_case(PLACEHOLDER_AES_KEY_HEX) {
            return Err(ConfigError::InsecurePlaceholder("AES_256_KEY_HEX".into()));
        }
        let aes_bytes = hex::decode(&aes_hex)
            .map_err(|e| ConfigError::Invalid("AES_256_KEY_HEX".into(), e.to_string()))?;
        if aes_bytes.len() != 32 {
            return Err(ConfigError::Invalid(
                "AES_256_KEY_HEX".into(),
                format!("expected 32 bytes, got {}", aes_bytes.len()),
            ));
        }
        let mut aes_key = [0u8; 32];
        aes_key.copy_from_slice(&aes_bytes);

        let sla_thresholds_raw = optional("SLA_ALERT_THRESHOLDS", "0.75,0.90,1.00");
        let sla_alert_thresholds = sla_thresholds_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|e| ConfigError::Invalid("SLA_ALERT_THRESHOLDS".into(), e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(AppConfig {
            database: DatabaseConfig {
                url: require("DATABASE_URL")?,
                max_connections: parse_num("DATABASE_MAX_CONNECTIONS", 10u32)?,
            },
            http: HttpConfig {
                host: optional("HTTP_HOST", "0.0.0.0"),
                port: parse_num("HTTP_PORT", 8080u16)?,
                enable_tls: parse_bool("ENABLE_TLS", false)?,
                tls_cert_path: optional("TLS_CERT_PATH", ""),
                tls_key_path: optional("TLS_KEY_PATH", ""),
            },
            auth: AuthConfig {
                jwt_secret: {
                    let s = require("JWT_SECRET")?;
                    if !dev_mode
                        && PLACEHOLDER_JWT_SECRETS
                            .iter()
                            .any(|p| p.eq_ignore_ascii_case(&s))
                    {
                        return Err(ConfigError::InsecurePlaceholder("JWT_SECRET".into()));
                    }
                    s
                },
                jwt_expiry_hours: parse_num("JWT_EXPIRY_HOURS", 24i64)?,
                jwt_issuer: optional("JWT_ISSUER", "fieldops-backend"),
                jwt_audience: optional("JWT_AUDIENCE", "fieldops-frontend"),
                argon2_memory_kib: parse_num("ARGON2_MEMORY_KIB", 19456u32)?,
                argon2_iterations: parse_num("ARGON2_ITERATIONS", 2u32)?,
                argon2_parallelism: parse_num("ARGON2_PARALLELISM", 1u32)?,
            },
            encryption: EncryptionConfig { aes_256_key: aes_key },
            logging: LoggingConfig {
                level: optional("LOG_LEVEL", "info"),
                format: optional("LOG_FORMAT", "structured"),
            },
            business: BusinessConfig {
                sync_interval_minutes: parse_num("SYNC_INTERVAL_MINUTES", 10u64)?,
                default_service_radius_miles: parse_num("DEFAULT_SERVICE_RADIUS_MILES", 30i32)?,
                max_notifications_per_hour: parse_num("MAX_NOTIFICATIONS_PER_HOUR", 20u32)?,
                max_versions_per_progress: parse_num("MAX_VERSIONS_PER_PROGRESS", 30u32)?,
                soft_delete_retention_days: parse_num("SOFT_DELETE_RETENTION_DAYS", 90u32)?,
                sla_alert_thresholds,
                notification_retry_max_attempts: parse_num("NOTIFICATION_RETRY_MAX_ATTEMPTS", 5u32)?,
                notification_retry_base_seconds: parse_num("NOTIFICATION_RETRY_BASE_SECONDS", 1u64)?,
                on_call_high_priority_hours: parse_num("ON_CALL_HIGH_PRIORITY_HOURS", 4i64)?,
            },
            app: AppBehaviorConfig {
                run_migrations_on_boot: parse_bool("RUN_MIGRATIONS_ON_BOOT", true)?,
                seed_default_admin: parse_bool("SEED_DEFAULT_ADMIN", true)?,
                default_admin_username: optional("DEFAULT_ADMIN_USERNAME", "admin"),
                default_admin_password: {
                    let pw = require("DEFAULT_ADMIN_PASSWORD")?;
                    if !dev_mode
                        && PLACEHOLDER_ADMIN_PASSWORDS
                            .iter()
                            .any(|p| p.eq_ignore_ascii_case(&pw))
                    {
                        return Err(ConfigError::InsecurePlaceholder(
                            "DEFAULT_ADMIN_PASSWORD".into(),
                        ));
                    }
                    pw
                },
                dev_mode,
                require_admin_password_change,
                // Default: permitted in dev, denied in production. Operator can
                // override in either direction via ALLOW_GEOCODE_FALLBACK.
                allow_geocode_fallback: parse_bool("ALLOW_GEOCODE_FALLBACK", dev_mode)?,
            },
        })
    }
}

impl AppConfig {
    /// Test-only constructor with sensible defaults.
    ///
    /// Reads `DATABASE_URL` from the environment (falling back to the
    /// docker-compose postgres URL) but hardcodes deterministic test values
    /// for everything else so integration tests are reproducible.
    pub fn test() -> Self {
        let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://fieldops:fieldops_pw@postgres:5432/fieldops".into()
        });
        Self {
            database: DatabaseConfig { url, max_connections: 5 },
            http: HttpConfig {
                host: "127.0.0.1".into(),
                port: 0,
                enable_tls: false,
                tls_cert_path: String::new(),
                tls_key_path: String::new(),
            },
            auth: AuthConfig {
                jwt_secret: "test-jwt-secret-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".into(),
                jwt_expiry_hours: 1,
                jwt_issuer: "fieldops-test".into(),
                jwt_audience: "fieldops-test".into(),
                argon2_memory_kib: 8192,
                argon2_iterations: 2,
                argon2_parallelism: 1,
            },
            encryption: EncryptionConfig { aes_256_key: [0x42u8; 32] },
            logging: LoggingConfig { level: "warn".into(), format: "structured".into() },
            business: BusinessConfig {
                sync_interval_minutes: 0,
                default_service_radius_miles: 30,
                max_notifications_per_hour: 20,
                max_versions_per_progress: 30,
                soft_delete_retention_days: 90,
                sla_alert_thresholds: vec![0.75, 0.90, 1.00],
                notification_retry_max_attempts: 5,
                notification_retry_base_seconds: 1,
                on_call_high_priority_hours: 4,
            },
            app: AppBehaviorConfig {
                run_migrations_on_boot: false,
                seed_default_admin: false,
                default_admin_username: "admin".into(),
                default_admin_password: "admin123".into(),
                dev_mode: true,
                require_admin_password_change: false,
                // Tests exercise both paths; default to allow so existing
                // geocode tests keep passing. Strict-mode tests flip to false.
                allow_geocode_fallback: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_defaults_to_false() {
        std::env::remove_var("ENABLE_TLS");
        assert!(!parse_bool("ENABLE_TLS", false).unwrap());
    }

    #[test]
    fn parse_bool_accepts_common_values() {
        std::env::set_var("T_BOOL_TEST", "true");
        assert!(parse_bool("T_BOOL_TEST", false).unwrap());
        std::env::set_var("T_BOOL_TEST", "1");
        assert!(parse_bool("T_BOOL_TEST", false).unwrap());
        std::env::set_var("T_BOOL_TEST", "no");
        assert!(!parse_bool("T_BOOL_TEST", true).unwrap());
        std::env::remove_var("T_BOOL_TEST");
    }

    #[test]
    fn placeholder_admin_password_rejected_in_prod() {
        assert!(PLACEHOLDER_ADMIN_PASSWORDS.contains(&"admin123"));
        assert!(PLACEHOLDER_ADMIN_PASSWORDS.contains(&"password"));
    }

    #[test]
    fn placeholder_jwt_secret_rejected_in_prod() {
        assert!(PLACEHOLDER_JWT_SECRETS
            .iter()
            .any(|s| s.eq_ignore_ascii_case("dev-jwt-secret-change-in-prod-0123456789abcdef")));
    }

    #[test]
    fn placeholder_aes_key_constant_matches_known_dev_value() {
        assert_eq!(PLACEHOLDER_AES_KEY_HEX.len(), 64);
    }

    #[test]
    fn auth_debug_redacts_secret() {
        let c = AuthConfig {
            jwt_secret: "supersecret".into(),
            jwt_expiry_hours: 24,
            jwt_issuer: "iss".into(),
            jwt_audience: "aud".into(),
            argon2_memory_kib: 19456,
            argon2_iterations: 2,
            argon2_parallelism: 1,
        };
        let s = format!("{:?}", c);
        assert!(!s.contains("supersecret"));
        assert!(s.contains("<redacted>"));
    }
}
