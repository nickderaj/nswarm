//! Versioned intent and deterministic SQLite-state parity harness.

use std::{collections::BTreeMap, path::Path};

use chrono::DateTime;
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Current machine-validated intent schema version.
pub const INTENT_SCHEMA_VERSION: u16 = 1;
/// Current committed v0 golden-snapshot schema version.
pub const GOLDEN_SCHEMA_VERSION: u16 = 1;

/// A transport-independent operation applied to both database copies.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParityIntent {
    /// Version of the committed intent contract.
    pub schema_version: u16,
    /// Exact intent variant.
    pub kind: IntentKind,
    /// Exact RFC-3339 timestamp supplied by the harness.
    pub at: String,
    /// IANA time zone configured in v0 when the intent was captured.
    pub time_zone: String,
    /// Positive finite kilograms.
    pub kilograms: f64,
}

impl ParityIntent {
    /// Parses and validates one committed intent document.
    ///
    /// # Errors
    ///
    /// Returns [`ParityError`] for malformed JSON or a contract violation.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ParityError> {
        let intent: Self = serde_json::from_slice(bytes)?;
        intent.validate()?;
        Ok(intent)
    }

    /// Validates schema version, timestamp, and domain values.
    ///
    /// # Errors
    ///
    /// Returns [`ParityError`] when the intent is not current or valid.
    pub fn validate(&self) -> Result<(), ParityError> {
        if self.schema_version != INTENT_SCHEMA_VERSION {
            return Err(ParityError::IntentSchemaVersion {
                expected: INTENT_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        DateTime::parse_from_rfc3339(&self.at).map_err(|_| ParityError::InvalidIntentTimestamp)?;
        jiff::tz::TimeZone::get(&self.time_zone).map_err(|_| ParityError::InvalidIntentTimeZone)?;
        if !self.kilograms.is_finite() || self.kilograms <= 0.0 {
            return Err(ParityError::InvalidIntentWeight);
        }
        Ok(())
    }
}

/// Version-1 intent kinds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    /// Log one fixed-time manual body weight.
    LogBodyWeight,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenSnapshot {
    schema_version: u16,
    source: GoldenSource,
    intent: ParityIntent,
    table_rows: BTreeMap<String, Vec<Vec<CellValue>>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenSource {
    commit: String,
    file: String,
    sha256: String,
    generator: String,
}

/// Builds the expected v0 state from an empty frozen fixture and a committed
/// golden row snapshot captured by the real v0 repository implementation.
///
/// # Errors
///
/// Returns [`ParityError`] when the golden document is invalid, does not match
/// the supplied intent, names an unknown table, or the fixture is not empty.
pub fn expected_v0_snapshot(
    empty_fixture_path: &Path,
    intent: &ParityIntent,
    golden_bytes: &[u8],
) -> Result<DatabaseSnapshot, ParityError> {
    intent.validate()?;
    let golden: GoldenSnapshot = serde_json::from_slice(golden_bytes)?;
    if golden.schema_version != GOLDEN_SCHEMA_VERSION {
        return Err(ParityError::GoldenSchemaVersion {
            expected: GOLDEN_SCHEMA_VERSION,
            actual: golden.schema_version,
        });
    }
    golden.intent.validate()?;
    if golden.intent != *intent {
        return Err(ParityError::GoldenIntentMismatch);
    }
    if golden.source.commit.is_empty()
        || golden.source.file.is_empty()
        || golden.source.sha256.len() != 64
        || golden.source.generator.is_empty()
    {
        return Err(ParityError::InvalidGoldenProvenance);
    }
    let mut snapshot = normalize_database(empty_fixture_path)?;
    if snapshot.tables.values().any(|table| !table.rows.is_empty()) {
        return Err(ParityError::GoldenFixtureNotEmpty);
    }
    for (table_name, mut rows) in golden.table_rows {
        let table = snapshot
            .tables
            .get_mut(&table_name)
            .ok_or_else(|| ParityError::UnknownGoldenTable(table_name.clone()))?;
        rows.sort();
        table.rows = rows;
    }
    Ok(snapshot)
}

/// Deterministic normalized representation of a complete `SQLite` database.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseSnapshot {
    /// `SQLite` `user_version`.
    pub schema_version: i64,
    /// Every application table, keyed deterministically by name.
    pub tables: BTreeMap<String, TableSnapshot>,
    /// Every index, view, and trigger, keyed by type and name.
    pub schema_objects: BTreeMap<String, SchemaObjectSnapshot>,
    /// Every foreign-key violation. A valid snapshot has none.
    pub foreign_key_violations: Vec<ForeignKeyViolation>,
}

/// One non-table object from `sqlite_schema`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SchemaObjectSnapshot {
    /// `SQLite` object kind (`index`, `view`, or `trigger`).
    pub kind: String,
    /// Object name.
    pub name: String,
    /// Table or view the object belongs to.
    pub table: String,
    /// Exact DDL, absent for `SQLite`'s implicit auto-indexes.
    pub sql: Option<String>,
}

/// Normalized schema and row state for one table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TableSnapshot {
    /// Exact table DDL from `sqlite_schema`.
    pub sql: String,
    /// Ordered column contract from `pragma_table_xinfo`.
    pub columns: Vec<ColumnSnapshot>,
    /// All rows, normalized and sorted.
    pub rows: Vec<Vec<CellValue>>,
}

/// One `SQLite` column definition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ColumnSnapshot {
    /// Declared column order.
    pub cid: i64,
    /// Column name.
    pub name: String,
    /// Declared `SQLite` type.
    pub declared_type: String,
    /// Whether the column carries `NOT NULL`.
    pub not_null: bool,
    /// Declared default expression, if any.
    pub default_value: Option<String>,
    /// Primary-key position.
    pub primary_key: i64,
    /// Hidden/generated marker from `table_xinfo`.
    pub hidden: i64,
}

/// Lossless deterministic `SQLite` cell representation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "storage", content = "value", rename_all = "snake_case")]
pub enum CellValue {
    /// `SQLite` NULL.
    Null,
    /// `SQLite` signed integer.
    Integer(i64),
    /// `SQLite` real represented by its exact IEEE-754 bits.
    Real(u64),
    /// UTF-8 `SQLite` text.
    Text(String),
    /// Non-UTF-8 `SQLite` text encoded as lowercase hexadecimal.
    NonUtf8Text(String),
    /// `SQLite` blob encoded as lowercase hexadecimal.
    Blob(String),
}

/// One row from `pragma_foreign_key_check`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ForeignKeyViolation {
    /// Child table containing the violation.
    pub table: String,
    /// Violating child rowid, when available.
    pub row_id: Option<i64>,
    /// Referenced parent table.
    pub parent: String,
    /// Foreign-key constraint index.
    pub constraint: i64,
}

/// Builds a deterministic schema-and-row snapshot from an existing `SQLite`
/// database, including invalid or drifted schemas so they can be diffed.
///
/// # Errors
///
/// Returns [`ParityError`] when `SQLite` cannot inspect the file.
pub fn normalize_database(path: &Path) -> Result<DatabaseSnapshot, ParityError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY.union(OpenFlags::SQLITE_OPEN_NO_MUTEX),
    )?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let schema_version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let mut tables = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_schema \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let table_rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for table_row in table_rows {
        let (name, sql) = table_row?;
        tables.insert(name.clone(), snapshot_table(&connection, &name, sql)?);
    }
    let mut schema_objects = BTreeMap::new();
    let mut object_statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema \
         WHERE type != 'table' ORDER BY type, name",
    )?;
    let object_rows = object_statement.query_map([], |row| {
        Ok(SchemaObjectSnapshot {
            kind: row.get(0)?,
            name: row.get(1)?,
            table: row.get(2)?,
            sql: row.get(3)?,
        })
    })?;
    for object in object_rows {
        let object = object?;
        schema_objects.insert(format!("{}/{}", object.kind, object.name), object);
    }
    let mut foreign_key_violations = Vec::new();
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = foreign_keys.query_map([], |row| {
        Ok(ForeignKeyViolation {
            table: row.get(0)?,
            row_id: row.get(1)?,
            parent: row.get(2)?,
            constraint: row.get(3)?,
        })
    })?;
    for row in rows {
        foreign_key_violations.push(row?);
    }
    foreign_key_violations.sort();
    Ok(DatabaseSnapshot {
        schema_version,
        tables,
        schema_objects,
        foreign_key_violations,
    })
}

fn snapshot_table(
    connection: &Connection,
    table: &str,
    sql: String,
) -> Result<TableSnapshot, rusqlite::Error> {
    let quoted = quote_identifier(table);
    let mut columns = Vec::new();
    let mut column_statement = connection.prepare(&format!("PRAGMA table_xinfo({quoted})"))?;
    let column_rows = column_statement.query_map([], |row| {
        Ok(ColumnSnapshot {
            cid: row.get(0)?,
            name: row.get(1)?,
            declared_type: row.get(2)?,
            not_null: row.get::<_, i64>(3)? != 0,
            default_value: row.get(4)?,
            primary_key: row.get(5)?,
            hidden: row.get(6)?,
        })
    })?;
    for column in column_rows {
        columns.push(column?);
    }
    let mut row_statement = connection.prepare(&format!("SELECT * FROM {quoted}"))?;
    let width = row_statement.column_count();
    let row_values = row_statement.query_map([], |row| {
        (0..width)
            .map(|index| Ok(normalize_cell(row.get_ref(index)?)))
            .collect::<Result<Vec<_>, _>>()
    })?;
    let mut rows = Vec::new();
    for row in row_values {
        rows.push(row?);
    }
    rows.sort();
    Ok(TableSnapshot { sql, columns, rows })
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn normalize_cell(value: ValueRef<'_>) -> CellValue {
    match value {
        ValueRef::Null => CellValue::Null,
        ValueRef::Integer(value) => CellValue::Integer(value),
        ValueRef::Real(value) => CellValue::Real(value.to_bits()),
        ValueRef::Text(value) => std::str::from_utf8(value).map_or_else(
            |_| CellValue::NonUtf8Text(hex(value)),
            |value| CellValue::Text(value.to_owned()),
        ),
        ValueRef::Blob(value) => CellValue::Blob(hex(value)),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    result
}

/// Explicit paths allowed to differ between snapshots.
///
/// Step 2 uses [`Self::empty`]; the type exists so future nondeterminism cannot
/// be silently ignored.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DifferenceAllowList {
    ignored_paths: Vec<String>,
}

impl DifferenceAllowList {
    /// Creates the preferred empty allow-list.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ignored_paths: Vec::new(),
        }
    }

    /// Creates an explicit allow-list. Each entry must be documented by the
    /// caller before use.
    #[must_use]
    pub const fn explicit(paths: Vec<String>) -> Self {
        Self {
            ignored_paths: paths,
        }
    }

    fn ignores(&self, path: &str) -> bool {
        self.ignored_paths
            .iter()
            .any(|ignored| ignored.as_str().eq(path))
    }
}

/// One deterministic structural state difference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StateDifference {
    /// JSON-pointer-like path to the mismatch.
    pub path: String,
    /// Baseline value, absent when v1 added structure.
    pub expected: Option<Value>,
    /// v1 value, absent when v1 removed structure.
    pub actual: Option<Value>,
}

/// Compares every normalized schema and row field.
///
/// # Errors
///
/// Returns [`ParityError`] only if snapshot serialization fails.
pub fn compare_snapshots(
    expected: &DatabaseSnapshot,
    actual: &DatabaseSnapshot,
    allow_list: &DifferenceAllowList,
) -> Result<Vec<StateDifference>, ParityError> {
    let expected = serde_json::to_value(expected)?;
    let actual = serde_json::to_value(actual)?;
    let mut differences = Vec::new();
    diff_values(
        "",
        Some(&expected),
        Some(&actual),
        allow_list,
        &mut differences,
    );
    Ok(differences)
}

fn diff_values(
    path: &str,
    expected: Option<&Value>,
    actual: Option<&Value>,
    allow_list: &DifferenceAllowList,
    differences: &mut Vec<StateDifference>,
) {
    if allow_list.ignores(path) || expected == actual {
        return;
    }
    match (expected, actual) {
        (Some(Value::Object(expected)), Some(Value::Object(actual))) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .collect::<std::collections::BTreeSet<_>>();
            for key in keys {
                diff_values(
                    &format!("{path}/{}", escape_pointer(key)),
                    expected.get(key),
                    actual.get(key),
                    allow_list,
                    differences,
                );
            }
        }
        (Some(Value::Array(expected)), Some(Value::Array(actual))) => {
            let length = expected.len().max(actual.len());
            for index in 0..length {
                diff_values(
                    &format!("{path}/{index}"),
                    expected.get(index),
                    actual.get(index),
                    allow_list,
                    differences,
                );
            }
        }
        _ => differences.push(StateDifference {
            path: if path.is_empty() {
                "/".to_owned()
            } else {
                path.to_owned()
            },
            expected: expected.cloned(),
            actual: actual.cloned(),
        }),
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

/// Parity intent, `SQLite`, and snapshot failures.
#[derive(Debug, Error)]
pub enum ParityError {
    /// Intent JSON does not match the typed schema.
    #[error("invalid parity intent JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Intent schema version is not supported.
    #[error("intent schema version mismatch: expected {expected}, found {actual}")]
    IntentSchemaVersion {
        /// Current version.
        expected: u16,
        /// Supplied version.
        actual: u16,
    },
    /// Fixed timestamp is not RFC-3339.
    #[error("intent timestamp must be RFC-3339")]
    InvalidIntentTimestamp,
    /// Configured time zone is not available in the IANA database.
    #[error("intent time_zone must be a valid IANA name")]
    InvalidIntentTimeZone,
    /// Fixed weight is not positive and finite.
    #[error("intent kilograms must be positive and finite")]
    InvalidIntentWeight,
    /// Golden snapshot schema version is not supported.
    #[error("golden schema version mismatch: expected {expected}, found {actual}")]
    GoldenSchemaVersion {
        /// Current version.
        expected: u16,
        /// Supplied version.
        actual: u16,
    },
    /// Golden intent and requested intent differ.
    #[error("golden snapshot intent does not match the requested intent")]
    GoldenIntentMismatch,
    /// Golden source metadata is incomplete or malformed.
    #[error("golden snapshot provenance is invalid")]
    InvalidGoldenProvenance,
    /// Golden rows must be layered onto an empty sanitized fixture.
    #[error("golden snapshot fixture must contain no application rows")]
    GoldenFixtureNotEmpty,
    /// Golden document names a table absent from the frozen schema.
    #[error("golden snapshot names unknown table {0}")]
    UnknownGoldenTable(String),
    /// `SQLite` could not apply or inspect state.
    #[error("parity SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
