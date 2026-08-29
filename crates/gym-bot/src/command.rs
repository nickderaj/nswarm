//! Transport-neutral gym command handling.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use botkit::UpdateKey;
use thiserror::Error;

use crate::{
    clock::Clock,
    database::{DatabaseError, open_existing},
};

const WEIGHT_USAGE: &str = "Usage: /weight <kg>";

/// Plain input accepted from any front-end adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandInput {
    /// Generic identity of the actor on the configured surface.
    pub actor_id: String,
    /// Generic idempotency key supplied by that surface.
    pub update: UpdateKey,
    /// Transport-neutral command text.
    pub text: String,
}

/// Reason a command intentionally produced no response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IgnoreReason {
    /// The actor does not match the configured single owner.
    NotOwner,
    /// The surface/external-id pair was already handled in this process.
    DuplicateUpdate,
}

/// Result of handling one transport-neutral command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandResult {
    /// Plain text for the adapter to deliver.
    Reply(String),
    /// An intentional no-response result.
    Ignored(IgnoreReason),
}

/// Minimal deterministic gym command service.
pub struct CommandService {
    owner_id: String,
    database_path: PathBuf,
    clock: Arc<dyn Clock>,
    processed: Mutex<HashSet<UpdateKey>>,
}

impl CommandService {
    /// Creates a command service against an existing v0 gym database copy.
    #[must_use]
    pub fn new(
        owner_id: impl Into<String>,
        database_path: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            owner_id: owner_id.into(),
            database_path: database_path.into(),
            clock,
            processed: Mutex::new(HashSet::new()),
        }
    }

    /// Returns the database path used by this service.
    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Handles one plain-text command.
    ///
    /// The owner check precedes idempotency, matching v0. Authorized malformed
    /// commands still consume their update key, also matching v0 behavior.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when duplicate tracking is unavailable or the
    /// database cannot apply an otherwise valid command.
    pub fn handle(&self, input: &CommandInput) -> Result<CommandResult, CommandError> {
        if input.actor_id != self.owner_id {
            return Ok(CommandResult::Ignored(IgnoreReason::NotOwner));
        }
        let mut processed = self
            .processed
            .lock()
            .map_err(|_| CommandError::IdempotencyUnavailable)?;
        if !processed.insert(input.update.clone()) {
            return Ok(CommandResult::Ignored(IgnoreReason::DuplicateUpdate));
        }
        drop(processed);

        let parsed = match parse_weight_command(&input.text) {
            Ok(parsed) => parsed,
            Err(WeightParseError::Usage) => {
                return Ok(CommandResult::Reply(WEIGHT_USAGE.to_owned()));
            }
        };
        let connection = open_existing(&self.database_path)?;
        connection.execute(
            "INSERT INTO body_metrics (date, metric, value, unit, source) \
             VALUES (?1, 'weight_kg', ?2, 'kg', 'manual')",
            (&self.clock.now_iso8601(), parsed.kilograms),
        )?;
        Ok(CommandResult::Reply(format!(
            "✅ Logged weight: {} kg",
            parsed.kilograms
        )))
    }
}

/// A validated body-weight command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightCommand {
    /// Positive, finite weight in kilograms.
    pub kilograms: f64,
}

/// Parses exactly `/weight <kg>` with the optional lowercase `kg` suffix used
/// by v0.
///
/// # Errors
///
/// Returns [`WeightParseError`] for blank, malformed, non-finite, zero, or
/// negative input.
pub fn parse_weight_command(text: &str) -> Result<WeightCommand, WeightParseError> {
    let mut tokens = text.split_whitespace();
    if tokens.next() != Some("/weight") {
        return Err(WeightParseError::Usage);
    }
    let raw = tokens.next().ok_or(WeightParseError::Usage)?;
    if tokens.next().is_some() {
        return Err(WeightParseError::Usage);
    }
    let numeric = raw.strip_suffix("kg").unwrap_or(raw);
    if numeric.is_empty() {
        return Err(WeightParseError::Usage);
    }
    let kilograms = numeric
        .parse::<f64>()
        .map_err(|_| WeightParseError::Usage)?;
    if !kilograms.is_finite() || kilograms <= 0.0 {
        return Err(WeightParseError::Usage);
    }
    Ok(WeightCommand { kilograms })
}

/// Weight command validation failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WeightParseError {
    /// The caller should receive the bounded command usage string.
    #[error("{WEIGHT_USAGE}")]
    Usage,
}

/// Command execution failure.
#[derive(Debug, Error)]
pub enum CommandError {
    /// The in-process idempotency guard was poisoned.
    #[error("command idempotency guard unavailable")]
    IdempotencyUnavailable,
    /// The gym database failed validation or access.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// A valid command could not be persisted.
    #[error("could not persist gym command: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
