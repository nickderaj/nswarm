//! Gym `SQLite` access without changing the frozen v0 schema.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use thiserror::Error;

/// The v0 gym schema version represented by the sanitized fixture.
pub const V0_GYM_SCHEMA_VERSION: i64 = 5;

/// Opens an existing gym database for command writes.
///
/// # Errors
///
/// Returns [`DatabaseError`] when the database is absent, unreadable, has a
/// different schema version.
pub fn open_existing(path: &Path) -> Result<Connection, DatabaseError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_and_validate(&connection)?;
    Ok(connection)
}

/// Opens an existing gym database read-only for MCP queries and snapshots.
///
/// # Errors
///
/// Returns [`DatabaseError`] when the database is absent, unreadable, has a
/// different schema version.
pub fn open_existing_read_only(path: &Path) -> Result<Connection, DatabaseError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_and_validate(&connection)?;
    Ok(connection)
}

fn configure_and_validate(connection: &Connection) -> Result<(), DatabaseError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != V0_GYM_SCHEMA_VERSION {
        return Err(DatabaseError::SchemaVersion {
            expected: V0_GYM_SCHEMA_VERSION,
            actual: version,
        });
    }
    Ok(())
}

/// Performs the full startup validation, including the potentially expensive
/// scan of every foreign-key-bearing table.
///
/// # Errors
///
/// Returns [`DatabaseError`] when the file, schema version, or foreign-key
/// state is invalid.
pub fn validate_existing(path: &Path) -> Result<(), DatabaseError> {
    let connection = open_existing_read_only(path)?;
    let violations: i64 =
        connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations != 0 {
        return Err(DatabaseError::ForeignKeyViolations(violations));
    }
    Ok(())
}

/// Gym database open and validation failures.
#[derive(Debug, Error)]
pub enum DatabaseError {
    /// `SQLite` rejected an open, pragma, or query operation.
    #[error("gym database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database is not the committed v0 gym schema version.
    #[error("gym schema version mismatch: expected {expected}, found {actual}")]
    SchemaVersion {
        /// Required fixture schema version.
        expected: i64,
        /// Version found in the database.
        actual: i64,
    },
    /// Existing rows violate the declared foreign keys.
    #[error("gym database has {0} foreign-key violation(s)")]
    ForeignKeyViolations(i64),
}
