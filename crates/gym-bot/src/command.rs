//! Transport-neutral gym command handling.

use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use botkit::UpdateKey;
use rusqlite::{Connection, OpenFlags, params};
use thiserror::Error;

use crate::{
    clock::Clock,
    database::{DatabaseError, open_existing, validate_existing},
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
    /// The surface/external-id pair was already handled, including before a restart.
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
    processed: Mutex<Connection>,
}

impl CommandService {
    /// Creates a command service with a durable generic idempotency sidecar.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the sidecar cannot be opened or initialized.
    pub fn new(
        owner_id: impl Into<String>,
        database_path: impl Into<PathBuf>,
        processed_updates_path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, CommandError> {
        let database_path = database_path.into();
        let processed_updates_path = processed_updates_path.as_ref();
        if processed_updates_path == database_path {
            return Err(CommandError::IdempotencyPathAliasesGymDatabase);
        }
        validate_existing(&database_path)?;
        if paths_alias(&database_path, processed_updates_path)
            .map_err(CommandError::IdempotencyPath)?
        {
            return Err(CommandError::IdempotencyPathAliasesGymDatabase);
        }
        let processed = Connection::open_with_flags(
            processed_updates_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        processed.busy_timeout(std::time::Duration::from_secs(5))?;
        processed.execute_batch(
            "CREATE TABLE IF NOT EXISTS processed_updates (\
                 surface TEXT NOT NULL, \
                 external_id TEXT NOT NULL, \
                 PRIMARY KEY (surface, external_id)\
             ) WITHOUT ROWID; \
             PRAGMA user_version=1;",
        )?;
        Ok(Self {
            owner_id: owner_id.into(),
            database_path,
            clock,
            processed: Mutex::new(processed),
        })
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
        let processed = self
            .processed
            .lock()
            .map_err(|_| CommandError::IdempotencyUnavailable)?;
        let inserted = processed.execute(
            "INSERT OR IGNORE INTO processed_updates (surface, external_id) VALUES (?1, ?2)",
            params![
                input.update.surface.as_str(),
                input.update.external_id.as_str()
            ],
        )?;
        if inserted == 0 {
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
            format_v0_general(parsed.kilograms)
        )))
    }
}

fn paths_alias(database_path: &Path, sidecar_path: &Path) -> Result<bool, std::io::Error> {
    let database = std::fs::canonicalize(database_path)?;
    let sidecar = match std::fs::canonicalize(sidecar_path) {
        Ok(sidecar) => sidecar,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if database == sidecar {
        return Ok(true);
    }
    let database_metadata = std::fs::metadata(database)?;
    let sidecar_metadata = std::fs::metadata(sidecar)?;
    Ok(database_metadata.dev() == sidecar_metadata.dev()
        && database_metadata.ino() == sidecar_metadata.ino())
}

fn format_v0_general(value: f64) -> String {
    const SIGNIFICANT_DIGITS: usize = 6;
    let scientific = format!("{value:.precision$e}", precision = SIGNIFICANT_DIGITS - 1);
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust scientific formatting always contains an exponent");
    let exponent: i32 = exponent
        .parse()
        .expect("Rust scientific formatting always emits an integer exponent");
    if !(-4..i32::try_from(SIGNIFICANT_DIGITS).expect("small precision")).contains(&exponent) {
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}e{exponent:+03}");
    }
    let decimal_places =
        usize::try_from(i32::try_from(SIGNIFICANT_DIGITS - 1).expect("small precision") - exponent)
            .expect("fixed notation has a non-negative decimal count");
    let fixed = format!("{value:.decimal_places$}");
    if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        fixed
    }
}

/// A validated body-weight command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightCommand {
    /// Positive, finite weight in kilograms.
    kilograms: f64,
}

impl WeightCommand {
    /// Returns the validated positive, finite weight in kilograms.
    #[must_use]
    pub const fn kilograms(&self) -> f64 {
        self.kilograms
    }
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
    /// The durable idempotency connection guard was poisoned.
    #[error("command idempotency guard unavailable")]
    IdempotencyUnavailable,
    /// The sidecar must never alias the frozen v0 gym database.
    #[error("processed-update sidecar must differ from the gym database")]
    IdempotencyPathAliasesGymDatabase,
    /// The filesystem boundary between the sidecar and gym database could not be resolved.
    #[error("could not resolve processed-update sidecar boundary: {0}")]
    IdempotencyPath(#[source] std::io::Error),
    /// The gym database failed validation or access.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// A valid command could not be persisted.
    #[error("could not persist gym command: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
