//! Secret-safe gym runtime configuration.

use std::{collections::HashMap, env, path::PathBuf};

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
        Ok(Self {
            telegram_token: token,
            owner_id,
            database_path,
            processed_updates_path: data.join("processed-updates.db"),
            socket_path: values
                .get("GYM_MCP_SOCKET")
                .map_or_else(|| PathBuf::from("/run/gym/mcp.sock"), PathBuf::from),
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
    /// Copied database validation failed.
    #[error("configured gym database is unavailable: {0}")]
    Database(crate::database::DatabaseError),
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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
        ]);
        let config = GymConfig::from_values(&values).expect("valid config");
        assert_eq!(config.owner_id, "1001");
        assert_eq!(config.timezone, "Europe/London");
        assert_eq!(config.database_path, directory.path().join("gym.db"));
        assert_eq!(
            config.processed_updates_path,
            directory.path().join("processed-updates.db")
        );
        let mut custom = values;
        custom.insert("TIMEZONE".to_owned(), "UTC".to_owned());
        assert_eq!(
            GymConfig::from_values(&custom)
                .expect("custom zone")
                .timezone,
            "UTC"
        );
    }

    #[test]
    fn environment_loader_executes_without_exposing_values() {
        let result = GymConfig::from_env();
        if let Err(error) = result {
            assert!(!error.to_string().contains("synthetic-secret"));
        }
    }
}
