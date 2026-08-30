//! Secret-safe gym runtime configuration.

use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf};

use thiserror::Error;

use crate::database::validate_existing;

/// Validated deployable gym configuration.
#[derive(Debug)]
pub struct GymConfig {
    /// New v1 Telegram token, never rendered in diagnostics.
    pub telegram_token: String,
    /// Single authorized Telegram actor.
    pub owner_id: String,
    /// Disposable copied frozen-schema database.
    pub database_path: PathBuf,
    /// v1-only durable update sidecar.
    pub processed_updates_path: PathBuf,
    /// Fleet-owned MCP socket.
    pub socket_path: PathBuf,
    /// Fleet-owned group expected on the MCP socket directory and socket.
    pub socket_group: String,
    /// Bearer credential accepted only by the Apple Health receiver.
    pub health_import_token: String,
    /// Explicit IP/port for the tailnet or loopback Health listener.
    pub health_bind_address: SocketAddr,
    /// Configured IANA time zone.
    pub timezone: String,
}

impl GymConfig {
    /// Loads the Fleet-rendered environment and validates non-secret boundaries.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for missing or invalid settings and storage.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_values(&env::vars().collect())
    }

    /// Loads an explicit setting map for supervised adapters and tests.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for missing or invalid settings and storage.
    pub fn from_map(values: &HashMap<String, String>) -> Result<Self, ConfigError> {
        Self::from_values(values)
    }

    fn from_values(values: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let token = required(values, "GYM_BOT_TOKEN")?;
        let owner_id = required(values, "OWNER_TELEGRAM_ID")?;
        owner_id.parse::<i64>().map_err(|_| ConfigError::OwnerId)?;
        let data = PathBuf::from(required(values, "GYM_DATA_DIR")?);
        if !data.is_absolute() {
            return Err(ConfigError::DataRoot);
        }
        let database_path = data.join("gym.db");
        validate_existing(&database_path).map_err(ConfigError::Database)?;
        let socket_path = PathBuf::from(required(values, "NSWARM_MCP_SOCKET")?);
        if !socket_path.is_absolute() {
            return Err(ConfigError::SocketPath);
        }
        let socket_group = required(values, "NSWARM_MCP_SOCKET_GROUP")?;
        if socket_group.len() > 64
            || !socket_group
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(ConfigError::SocketGroup);
        }
        let health_import_token = required(values, "HEALTH_IMPORT_TOKEN")?;
        let health_host = values
            .get("HEALTH_BIND_HOST")
            .map_or("127.0.0.1", String::as_str)
            .parse::<std::net::IpAddr>()
            .map_err(|_| ConfigError::HealthHost)?;
        if !health_host.is_loopback() {
            return Err(ConfigError::HealthHost);
        }
        let health_port = values
            .get("HEALTH_BIND_PORT")
            .map_or("8090", String::as_str)
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or(ConfigError::HealthPort)?;
        Ok(Self {
            telegram_token: token,
            owner_id,
            database_path,
            processed_updates_path: data.join("processed-updates.db"),
            socket_path,
            socket_group,
            health_import_token,
            health_bind_address: SocketAddr::new(health_host, health_port),
            timezone: values
                .get("TIMEZONE")
                .cloned()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Europe/London".to_owned()),
        })
    }
}

fn required(values: &HashMap<String, String>, name: &'static str) -> Result<String, ConfigError> {
    values
        .get(name)
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing(name))
}

/// Secret-safe startup failure.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// A required environment variable was absent; its value is never printed.
    #[error("required gym setting {0} is missing")]
    Missing(&'static str),
    /// Owner identity must be numeric.
    #[error("OWNER_TELEGRAM_ID must be an integer")]
    OwnerId,
    /// Data roots must be explicit absolute paths.
    #[error("GYM_DATA_DIR must be an absolute path")]
    DataRoot,
    /// Fleet socket paths must be absolute.
    #[error("NSWARM_MCP_SOCKET must be an absolute path")]
    SocketPath,
    /// Fleet socket groups use the portable service-identity alphabet.
    #[error("NSWARM_MCP_SOCKET_GROUP must be a portable group name")]
    SocketGroup,
    /// Health listeners bind only explicit loopback IP addresses.
    #[error("HEALTH_BIND_HOST must be a loopback IP address")]
    HealthHost,
    /// Health listener ports use the non-zero TCP port range.
    #[error("HEALTH_BIND_PORT must be an integer from 1 to 65535")]
    HealthPort,
    /// Copied database validation failed.
    #[error("configured gym database is unavailable: {0}")]
    Database(crate::database::DatabaseError),
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::{ConfigError, GymConfig};

    #[test]
    fn configuration_errors_are_secret_safe() {
        let mut values = HashMap::new();
        assert!(matches!(
            GymConfig::from_values(&values),
            Err(ConfigError::Missing("GYM_BOT_TOKEN"))
        ));
        values.insert("GYM_BOT_TOKEN".to_owned(), "synthetic-secret".to_owned());
        values.insert("OWNER_TELEGRAM_ID".to_owned(), "not-an-id".to_owned());
        values.insert("GYM_DATA_DIR".to_owned(), "relative".to_owned());
        let error = GymConfig::from_values(&values).expect_err("invalid owner");
        assert!(matches!(error, ConfigError::OwnerId));
        assert!(!error.to_string().contains("synthetic-secret"));
        values.insert("OWNER_TELEGRAM_ID".to_owned(), "1001".to_owned());
        assert!(matches!(
            GymConfig::from_values(&values),
            Err(ConfigError::DataRoot)
        ));
    }

    #[test]
    fn valid_values_load_defaults_and_paths() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/gym/v0-gym-v5.sqlite3"
            ),
            directory.path().join("gym.db"),
        )
        .expect("fixture");
        let values = HashMap::from([
            ("GYM_BOT_TOKEN".to_owned(), "synthetic".to_owned()),
            ("OWNER_TELEGRAM_ID".to_owned(), "1001".to_owned()),
            (
                "GYM_DATA_DIR".to_owned(),
                directory.path().display().to_string(),
            ),
            (
                "NSWARM_MCP_SOCKET".to_owned(),
                "/run/gym/mcp.sock".to_owned(),
            ),
            (
                "NSWARM_MCP_SOCKET_GROUP".to_owned(),
                "gym-access".to_owned(),
            ),
            (
                "HEALTH_IMPORT_TOKEN".to_owned(),
                "synthetic-health-secret".to_owned(),
            ),
        ]);
        let config = GymConfig::from_values(&values).expect("valid config");
        assert_eq!(config.owner_id, "1001");
        assert_eq!(config.timezone, "Europe/London");
        assert_eq!(config.database_path, directory.path().join("gym.db"));
        assert_eq!(config.socket_path, PathBuf::from("/run/gym/mcp.sock"));
        assert_eq!(config.socket_group, "gym-access");
        assert_eq!(config.health_bind_address.to_string(), "127.0.0.1:8090");
        assert_eq!(
            config.processed_updates_path,
            directory.path().join("processed-updates.db")
        );
        let mut custom = values;
        custom.insert("TIMEZONE".to_owned(), "UTC".to_owned());
        custom.insert("HEALTH_BIND_HOST".to_owned(), "::1".to_owned());
        custom.insert("HEALTH_BIND_PORT".to_owned(), "9000".to_owned());
        assert_eq!(
            GymConfig::from_values(&custom)
                .expect("custom zone")
                .timezone,
            "UTC"
        );
        assert_eq!(
            GymConfig::from_values(&custom)
                .expect("custom Health bind")
                .health_bind_address
                .to_string(),
            "[::1]:9000"
        );
        custom.insert("HEALTH_BIND_HOST".to_owned(), "100.64.0.10".to_owned());
        assert!(matches!(
            GymConfig::from_values(&custom),
            Err(ConfigError::HealthHost)
        ));
    }

    #[test]
    fn environment_loader_executes_without_exposing_values() {
        let error = GymConfig::from_env().expect_err("test environment has no gym token");
        assert!(!error.to_string().contains("synthetic-secret"));
    }
}
