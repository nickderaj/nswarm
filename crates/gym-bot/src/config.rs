//! Secret-safe gym runtime configuration.

use std::{collections::HashMap, env, path::PathBuf};

use thiserror::Error;

use crate::database::validate_existing;

/// Validated deployable gym configuration.
#[derive(Debug)]
pub struct GymConfig {
    /// Disposable copied frozen-schema database.
    pub database_path: PathBuf,
    /// Fleet-owned MCP socket.
    pub socket_path: PathBuf,
    /// Fleet-owned group expected on the MCP socket directory and socket.
    pub socket_group: String,
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
        Ok(Self {
            database_path,
            socket_path,
            socket_group,
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
    /// Data roots must be explicit absolute paths.
    #[error("GYM_DATA_DIR must be an absolute path")]
    DataRoot,
    /// Fleet socket paths must be absolute.
    #[error("NSWARM_MCP_SOCKET must be an absolute path")]
    SocketPath,
    /// Fleet socket groups use the portable service-identity alphabet.
    #[error("NSWARM_MCP_SOCKET_GROUP must be a portable group name")]
    SocketGroup,
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
            Err(ConfigError::Missing("GYM_DATA_DIR"))
        ));
        values.insert("GYM_DATA_DIR".to_owned(), "relative".to_owned());
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
        ]);
        let config = GymConfig::from_values(&values).expect("valid config");
        assert_eq!(config.timezone, "Europe/London");
        assert_eq!(config.database_path, directory.path().join("gym.db"));
        assert_eq!(config.socket_path, PathBuf::from("/run/gym/mcp.sock"));
        assert_eq!(config.socket_group, "gym-access");
        let mut custom = values;
        custom.insert("TIMEZONE".to_owned(), "UTC".to_owned());
        assert_eq!(
            GymConfig::from_values(&custom)
                .expect("custom zone")
                .timezone,
            "UTC"
        );
        custom.insert("NSWARM_MCP_SOCKET".to_owned(), "relative.sock".to_owned());
        assert!(matches!(
            GymConfig::from_values(&custom),
            Err(ConfigError::SocketPath)
        ));
        custom.insert(
            "NSWARM_MCP_SOCKET".to_owned(),
            "/run/gym/mcp.sock".to_owned(),
        );
        for group in ["Gym Access", &"a".repeat(65)] {
            custom.insert("NSWARM_MCP_SOCKET_GROUP".to_owned(), group.to_owned());
            assert!(matches!(
                GymConfig::from_values(&custom),
                Err(ConfigError::SocketGroup)
            ));
        }
        custom.insert("NSWARM_MCP_SOCKET_GROUP".to_owned(), "a".repeat(64));
        assert!(GymConfig::from_values(&custom).is_ok());
    }

    #[test]
    fn environment_loader_executes_without_exposing_values() {
        let error = GymConfig::from_env().expect_err("test environment has no gym data root");
        assert!(!error.to_string().contains("synthetic-secret"));
    }
}
