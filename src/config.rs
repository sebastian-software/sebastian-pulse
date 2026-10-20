use std::collections::HashMap;
use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgSslMode};
use thiserror::Error;
use url::Url;

pub type Environment = HashMap<String, String>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purpose {
    Runtime,
    Migration,
}

pub struct RuntimeConfig {
    pub port: u16,
    pub release: String,
    pub database: PgConnectOptions,
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeConfig")
            .field("port", &self.port)
            .field("release", &self.release)
            .field("database", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigurationError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{0} must be a PostgreSQL URL naming a host and database")]
    InvalidPostgresUrl(&'static str),
    #[error("Use DATABASE_TLS and DATABASE_CA_CERT instead of URL SSL options")]
    InlineSslOptions,
    #[error("Unsupported PostgreSQL URL parameter")]
    UnsupportedUrlParameter,
    #[error("Disabling database TLS requires APP_ENV=local or test")]
    PlaintextEnvironment,
    #[error("DATABASE_TLS must be verify-full or disable")]
    InvalidTlsMode,
    #[error("PORT must be 1-65535")]
    InvalidPort,
    #[error("RELEASE_ID must be a safe release identifier")]
    InvalidRelease,
    #[error("Do not provide migration credentials to the runtime")]
    RuntimeHasMigrationCredentials,
}

pub fn runtime_config(env: &Environment) -> Result<RuntimeConfig, ConfigurationError> {
    let port = match env.get("PORT") {
        Some(value) => value.parse::<u16>().ok().filter(|port| *port > 0),
        None => Some(3000),
    }
    .ok_or(ConfigurationError::InvalidPort)?;

    let release = env.get("RELEASE_ID").map(String::as_str).unwrap_or("local");
    if release.is_empty()
        || release.len() > 128
        || !release
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ConfigurationError::InvalidRelease);
    }

    if env
        .get("DATABASE_MIGRATION_URL")
        .is_some_and(|value| !value.is_empty())
    {
        return Err(ConfigurationError::RuntimeHasMigrationCredentials);
    }

    Ok(RuntimeConfig {
        port,
        release: release.to_owned(),
        database: database_config(env, Purpose::Runtime)?,
    })
}

pub fn database_config(
    env: &Environment,
    purpose: Purpose,
) -> Result<PgConnectOptions, ConfigurationError> {
    let key = match purpose {
        Purpose::Runtime => "DATABASE_URL",
        Purpose::Migration => "DATABASE_MIGRATION_URL",
    };
    let value = env
        .get(key)
        .filter(|value| !value.is_empty())
        .ok_or(ConfigurationError::Missing(key))?;

    let url = Url::parse(value).map_err(|_| ConfigurationError::InvalidPostgresUrl(key))?;
    let database_path = url
        .path()
        .strip_prefix('/')
        .filter(|database| !database.is_empty() && !database.starts_with('/'));
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.host_str().is_none()
        || database_path.is_none()
    {
        return Err(ConfigurationError::InvalidPostgresUrl(key));
    }

    for (name, _) in url.query_pairs() {
        if name.to_ascii_lowercase().starts_with("ssl") {
            return Err(ConfigurationError::InlineSslOptions);
        }
        if !supported_connection_parameter(&name) {
            // SQLx logs ignored URL parameters, including their values. Reject
            // them here so a typo or secret-bearing value cannot reach that log.
            return Err(ConfigurationError::UnsupportedUrlParameter);
        }
    }

    let mode = env
        .get("DATABASE_TLS")
        .map(String::as_str)
        .unwrap_or("verify-full");
    let (ssl_mode, use_custom_ca) = match mode {
        "verify-full" => (PgSslMode::VerifyFull, true),
        "disable"
            if matches!(
                env.get("APP_ENV").map(String::as_str),
                Some("local" | "test")
            ) =>
        {
            (PgSslMode::Disable, false)
        }
        "disable" => return Err(ConfigurationError::PlaintextEnvironment),
        _ => return Err(ConfigurationError::InvalidTlsMode),
    };

    // Parse connection details only after validating the URL ourselves; never
    // propagate a parser error that could contain the credential-bearing URL.
    let mut options = PgConnectOptions::from_str(value)
        .map_err(|_| ConfigurationError::InvalidPostgresUrl(key))?
        .ssl_mode(ssl_mode);

    if use_custom_ca
        && let Some(certificate) = env
            .get("DATABASE_CA_CERT")
            .filter(|certificate| !certificate.is_empty())
    {
        options = options.ssl_root_cert_from_pem(certificate.as_bytes().to_vec());
    }

    Ok(options)
}

fn supported_connection_parameter(name: &str) -> bool {
    matches!(
        name,
        "statement-cache-capacity"
            | "host"
            | "hostaddr"
            | "port"
            | "dbname"
            | "user"
            | "password"
            | "application_name"
            | "options"
    ) || name
        .strip_prefix("options[")
        .and_then(|name| name.strip_suffix(']'))
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::ConnectOptions;

    const URL: &str = "postgresql://service:secret-fixture@db.example/analytics";

    fn env(entries: &[(&str, &str)]) -> Environment {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn database_error(env: &Environment, purpose: Purpose) -> ConfigurationError {
        match database_config(env, purpose) {
            Err(error) => error,
            Ok(_) => panic!("expected database configuration to fail"),
        }
    }

    fn runtime_error(env: &Environment) -> ConfigurationError {
        match runtime_config(env) {
            Err(error) => error,
            Ok(_) => panic!("expected runtime configuration to fail"),
        }
    }

    #[test]
    fn database_connections_use_full_tls_verification_by_default() {
        let options = database_config(&env(&[("DATABASE_URL", URL)]), Purpose::Runtime).unwrap();
        let parsed = options.to_url_lossy();
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(name, _)| name == "sslmode")
                .unwrap()
                .1,
            "verify-full"
        );

        let with_ca = database_config(
            &env(&[("DATABASE_URL", URL), ("DATABASE_CA_CERT", "fixture-ca")]),
            Purpose::Runtime,
        )
        .unwrap();
        assert_eq!(
            with_ca
                .to_url_lossy()
                .query_pairs()
                .find(|(name, _)| name == "sslmode")
                .unwrap()
                .1,
            "verify-full"
        );
    }

    #[test]
    fn plaintext_requires_an_explicit_local_or_test_environment() {
        for app_env in [None, Some("production"), Some("preview")] {
            let mut entries = vec![("DATABASE_URL", URL), ("DATABASE_TLS", "disable")];
            if let Some(app_env) = app_env {
                entries.push(("APP_ENV", app_env));
            }
            assert_eq!(
                database_error(&env(&entries), Purpose::Runtime),
                ConfigurationError::PlaintextEnvironment
            );
        }

        for app_env in ["local", "test"] {
            let options = database_config(
                &env(&[
                    ("DATABASE_URL", URL),
                    ("DATABASE_TLS", "disable"),
                    ("APP_ENV", app_env),
                ]),
                Purpose::Runtime,
            )
            .unwrap();
            assert_eq!(
                options
                    .to_url_lossy()
                    .query_pairs()
                    .find(|(name, _)| name == "sslmode")
                    .unwrap()
                    .1,
                "disable"
            );
        }
    }

    #[test]
    fn tls_url_parameters_are_rejected_even_when_encoded_or_mixed_case() {
        for parameter in [
            "sslmode=disable",
            "sslmode=no-verify",
            "ssl=false",
            "sslcert=anything",
            "SSLmode=disable",
            "%73slmode=disable",
        ] {
            let url = format!("{URL}?{parameter}");
            let error = database_error(&env(&[("DATABASE_URL", &url)]), Purpose::Runtime);
            assert_eq!(error, ConfigurationError::InlineSslOptions);
            assert!(!error.to_string().contains("secret-fixture"));
        }
    }

    #[test]
    fn migration_and_runtime_urls_are_selected_by_purpose() {
        let options = database_config(
            &env(&[
                (
                    "DATABASE_URL",
                    "postgresql://runtime:secret@db.example/analytics",
                ),
                (
                    "DATABASE_MIGRATION_URL",
                    "postgresql://migrator:other-secret@db.example/analytics",
                ),
            ]),
            Purpose::Migration,
        )
        .unwrap();
        assert_eq!(options.get_username(), "migrator");

        let error = runtime_error(&env(&[
            ("DATABASE_URL", URL),
            ("DATABASE_MIGRATION_URL", "postgresql://secret"),
        ]));
        assert_eq!(error, ConfigurationError::RuntimeHasMigrationCredentials);
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn malformed_database_urls_fail_without_echoing_the_input() {
        for value in [
            "secret-fixture",
            "https://service:secret-fixture@db.example/analytics",
            "postgresql://service:secret-fixture@/analytics",
            "postgresql://service:secret-fixture@db.example/",
            "postgresql://service:secret-fixture@db.example//analytics",
        ] {
            let error = database_error(&env(&[("DATABASE_URL", value)]), Purpose::Runtime);
            assert_eq!(
                error,
                ConfigurationError::InvalidPostgresUrl("DATABASE_URL")
            );
            assert!(!error.to_string().contains("secret-fixture"));
        }

        assert_eq!(
            database_error(&Environment::new(), Purpose::Runtime),
            ConfigurationError::Missing("DATABASE_URL")
        );
    }

    #[test]
    fn runtime_port_and_release_are_bounded_and_safe() {
        let config = runtime_config(&env(&[("DATABASE_URL", URL)])).unwrap();
        assert_eq!(config.port, 3000);
        assert_eq!(config.release, "local");

        for port in ["0", "65536", "-1", "3000.5", "bad"] {
            assert_eq!(
                runtime_error(&env(&[("DATABASE_URL", URL), ("PORT", port)])),
                ConfigurationError::InvalidPort
            );
        }
        for release in ["", "unsafe\nvalue", "has space", "é"] {
            assert_eq!(
                runtime_error(&env(&[("DATABASE_URL", URL), ("RELEASE_ID", release)])),
                ConfigurationError::InvalidRelease
            );
        }
        let config = runtime_config(&env(&[
            ("DATABASE_URL", URL),
            ("PORT", "65535"),
            ("RELEASE_ID", "v1.2_rc-4"),
        ]))
        .unwrap();
        assert_eq!(config.port, 65535);
        assert_eq!(config.release, "v1.2_rc-4");
    }

    #[test]
    fn unsupported_url_parameters_are_rejected_without_echoing_values() {
        let url = format!("{URL}?unknown=secret-fixture");
        let error = database_error(&env(&[("DATABASE_URL", &url)]), Purpose::Runtime);
        assert_eq!(error, ConfigurationError::UnsupportedUrlParameter);
        assert!(!error.to_string().contains("secret-fixture"));
    }

    #[test]
    fn tls_mode_is_closed_and_exact() {
        for mode in ["require", "verify-ca", "DISABLE", ""] {
            assert_eq!(
                database_error(
                    &env(&[("DATABASE_URL", URL), ("DATABASE_TLS", mode)]),
                    Purpose::Runtime
                ),
                ConfigurationError::InvalidTlsMode
            );
        }
    }
}
