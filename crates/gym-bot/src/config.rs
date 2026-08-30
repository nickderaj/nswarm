//! Secret-safe gym runtime configuration.

use std::{env, path::PathBuf};

use thiserror::Error;

use crate::database::validate_existing;

/// Validated deployable gym configuration.
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
        let token = required("GYM_BOT_TOKEN")?;
        let owner_id = required("OWNER_TELEGRAM_ID")?;
        owner_id.parse::<i64>().map_err(|_| ConfigError::OwnerId)?;
        let data = PathBuf::from(required("GYM_DATA_DIR")?);
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
            socket_path: PathBuf::from("/run/gym/mcp.sock"),
            timezone: env::var("TIMEZONE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Europe/London".to_owned()),
        })
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
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
