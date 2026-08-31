//! Strict Apple Health import contract and transactional persistence.

use std::path::{Path, PathBuf};

use chrono::DateTime;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::database::{DatabaseError, open_existing};

/// One strict Apple Health HTTP payload.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthPayload {
    /// Workouts supplied by the phone exporter.
    #[serde(default)]
    pub workouts: Vec<HealthWorkout>,
    /// Body or recovery metrics supplied by the phone exporter.
    #[serde(default)]
    pub metrics: Vec<HealthMetric>,
}

/// One imported workout.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthWorkout {
    /// Stable source identity used for replay deduplication.
    pub external_id: String,
    /// RFC-3339 start time with an explicit offset.
    pub started_at: String,
    /// Human-readable activity.
    pub activity: String,
    /// Positive duration in seconds.
    pub duration_s: f64,
    /// Optional non-negative distance in metres.
    pub distance_m: Option<f64>,
    /// Optional positive average heart rate.
    pub avg_hr: Option<u16>,
    /// Optional interval splits.
    #[serde(default)]
    pub splits: Vec<HealthSplit>,
    /// Optional per-sample heart-rate series.
    #[serde(default)]
    pub hr_samples: Vec<HeartRateSample>,
}

/// One imported body/recovery metric.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthMetric {
    /// Stable source identity.
    pub external_id: String,
    /// RFC-3339 timestamp with an explicit offset.
    pub at: String,
    /// Reviewed metric name.
    pub metric: String,
    /// Finite, non-negative value.
    pub value: f64,
    /// Non-empty unit label.
    pub unit: String,
}

/// One workout interval.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthSplit {
    /// Optional distance in metres.
    pub distance_m: Option<f64>,
    /// Optional duration in seconds.
    pub duration_s: Option<f64>,
    /// Optional average heart rate.
    pub avg_hr: Option<u16>,
}

/// One heart-rate reading.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeartRateSample {
    /// RFC-3339 reading time.
    pub at: String,
    /// Positive beats per minute.
    pub bpm: u16,
}

/// Import counts suitable for the receiver response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportResult {
    /// Newly inserted source records.
    pub inserted: usize,
    /// Source records already present.
    pub duplicates: usize,
}

/// Deterministic Health importer.
pub struct HealthImporter {
    database_path: PathBuf,
}

impl HealthImporter {
    /// Creates an importer against an existing frozen-schema database.
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
    }

    /// Parses and imports one strict payload transactionally.
    ///
    /// # Errors
    ///
    /// Returns [`HealthError`] for malformed input, validation failure, or
    /// storage failure. No partial payload is committed.
    pub fn import_json(&self, input: &[u8]) -> Result<ImportResult, HealthError> {
        if input.len() > 1_048_576 {
            return Err(HealthError::PayloadTooLarge);
        }
        let payload: HealthPayload = serde_json::from_slice(input)?;
        validate_payload(&payload)?;
        import_payload(&self.database_path, &payload)
    }
}

fn validate_payload(payload: &HealthPayload) -> Result<(), HealthError> {
    if payload.workouts.len() > 100 {
        return Err(HealthError::Invalid("payload item limit exceeded"));
    }
    if payload.metrics.len() > 500 {
        return Err(HealthError::Invalid("payload item limit exceeded"));
    }
    for workout in &payload.workouts {
        require_identifier(&workout.external_id)?;
        require_text(&workout.activity, 100)?;
        parse_timestamp(&workout.started_at)?;
        if !workout.duration_s.is_finite() || workout.duration_s <= 0.0 {
            return Err(HealthError::Invalid(
                "duration_s must be positive and finite",
            ));
        }
        if workout
            .distance_m
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err(HealthError::Invalid(
                "distance_m must be non-negative and finite",
            ));
        }
        if workout.avg_hr.is_some_and(|value| value == 0)
            || workout.splits.len() > 1_000
            || workout.hr_samples.len() > 5_000
        {
            return Err(HealthError::Invalid("workout sample limits are invalid"));
        }
        for sample in &workout.hr_samples {
            parse_timestamp(&sample.at)?;
            if sample.bpm == 0 {
                return Err(HealthError::Invalid("heart rate must be positive"));
            }
        }
    }
    for metric in &payload.metrics {
        require_identifier(&metric.external_id)?;
        parse_timestamp(&metric.at)?;
        if !matches!(
            metric.metric.as_str(),
            "hrv_ms" | "sleep_s" | "resting_hr" | "vo2max"
        ) {
            return Err(HealthError::Invalid("unsupported Health metric"));
        }
        if !metric.value.is_finite() || metric.value < 0.0 || metric.unit.trim().is_empty() {
            return Err(HealthError::Invalid(
                "Health metric value or unit is invalid",
            ));
        }
    }
    Ok(())
}

fn import_payload(path: &Path, payload: &HealthPayload) -> Result<ImportResult, HealthError> {
    let mut connection = open_existing(path)?;
    let transaction = connection.transaction()?;
    let mut inserted = 0;
    let mut duplicates = 0;
    for workout in &payload.workouts {
        if source_exists(&transaction, &workout.external_id)? {
            duplicates += 1;
            continue;
        }
        let name = workout
            .activity
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let existing = matching_manual(&transaction, &name, workout)?;
        let session_id =
            if let Some(session_id) = existing {
                transaction.execute(
                    "UPDATE sessions SET source='apple_health' WHERE id=?1",
                    [session_id],
                )?;
                transaction.execute(
                "UPDATE efforts SET duration_s=?1,distance_m=?2,avg_hr=?3 WHERE session_item_id IN \
                 (SELECT id FROM session_items WHERE session_id=?4)",
                params![workout.duration_s, workout.distance_m, workout.avg_hr, session_id],
            )?;
                session_id
            } else {
                insert_workout(&transaction, &name, workout)?
            };
        let item_id: i64 = transaction.query_row(
            "SELECT id FROM session_items WHERE session_id=?1 ORDER BY position LIMIT 1",
            [session_id],
            |row| row.get(0),
        )?;
        for (position, split) in workout.splits.iter().enumerate() {
            transaction.execute(
                "INSERT INTO effort_splits (session_item_id,position,distance_m,duration_s,avg_hr) VALUES (?1,?2,?3,?4,?5)",
                params![item_id, i64::try_from(position + 1).expect("split limit fits i64"), split.distance_m, split.duration_s, split.avg_hr],
            )?;
        }
        for sample in &workout.hr_samples {
            transaction.execute(
                "INSERT INTO hr_samples (session_item_id,at,bpm) VALUES (?1,?2,?3)",
                params![item_id, sample.at, sample.bpm],
            )?;
        }
        transaction.execute(
            "INSERT INTO external_activities (source,external_id,session_id,payload) VALUES ('apple_health',?1,?2,?3)",
            params![workout.external_id, session_id, serde_json::to_string(workout)?],
        )?;
        inserted += 1;
    }
    for metric in &payload.metrics {
        if source_exists(&transaction, &metric.external_id)? {
            duplicates += 1;
            continue;
        }
        transaction.execute(
            "INSERT INTO body_metrics (date,metric,value,unit,source) VALUES (?1,?2,?3,?4,'apple_health')",
            params![metric.at, metric.metric, metric.value, metric.unit],
        )?;
        transaction.execute(
            "INSERT INTO external_activities (source,external_id,payload) VALUES ('apple_health',?1,?2)",
            params![metric.external_id, serde_json::to_string(metric)?],
        )?;
        inserted += 1;
    }
    transaction.commit()?;
    Ok(ImportResult {
        inserted,
        duplicates,
    })
}

fn source_exists(
    transaction: &rusqlite::Transaction<'_>,
    external_id: &str,
) -> rusqlite::Result<bool> {
    Ok(transaction
        .query_row(
            "SELECT 1 FROM external_activities WHERE source='apple_health' AND external_id=?1",
            [external_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn matching_manual(
    transaction: &rusqlite::Transaction<'_>,
    name: &str,
    workout: &HealthWorkout,
) -> Result<Option<i64>, HealthError> {
    let instant = parse_timestamp(&workout.started_at)?;
    let start = (instant - chrono::Duration::minutes(30)).to_rfc3339();
    let end = (instant + chrono::Duration::minutes(30)).to_rfc3339();
    let mut statement = transaction.prepare(
        "SELECT s.id,e.distance_m FROM sessions s JOIN session_items i ON i.session_id=s.id \
         JOIN movements m ON m.id=i.movement_id JOIN efforts e ON e.session_item_id=i.id \
         WHERE s.source='manual' AND m.modality='cardio' AND m.name=?1 AND s.started_at BETWEEN ?2 AND ?3 \
         ORDER BY s.started_at DESC",
    )?;
    let mut rows = statement.query(params![name, start, end])?;
    while let Some(row) = rows.next()? {
        let existing: Option<f64> = row.get(1)?;
        if workout.distance_m.is_none()
            || existing.is_none()
            || workout.distance_m.is_some_and(|distance| {
                (existing.unwrap_or_default() - distance).abs() <= distance * 0.1
            })
        {
            return Ok(Some(row.get(0)?));
        }
    }
    Ok(None)
}

fn insert_workout(
    transaction: &rusqlite::Transaction<'_>,
    name: &str,
    workout: &HealthWorkout,
) -> Result<i64, HealthError> {
    transaction.execute(
        "INSERT INTO sessions (started_at,kind,source) VALUES (?1,'cardio','apple_health')",
        [&workout.started_at],
    )?;
    let session_id = transaction.last_insert_rowid();
    transaction.execute("INSERT INTO movements (name,display_name,modality) VALUES (?1,?2,'cardio') ON CONFLICT(name) DO NOTHING", params![name, workout.activity])?;
    let movement_id: i64 =
        transaction.query_row("SELECT id FROM movements WHERE name=?1", [name], |row| {
            row.get(0)
        })?;
    transaction.execute(
        "INSERT INTO session_items (session_id,position,movement_id) VALUES (?1,1,?2)",
        params![session_id, movement_id],
    )?;
    let item_id = transaction.last_insert_rowid();
    transaction.execute("INSERT INTO efforts (session_item_id,position,duration_s,distance_m,avg_hr) VALUES (?1,1,?2,?3,?4)", params![item_id,workout.duration_s,workout.distance_m,workout.avg_hr])?;
    Ok(session_id)
}

fn parse_timestamp(value: &str) -> Result<DateTime<chrono::FixedOffset>, HealthError> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| HealthError::Invalid("timestamps must be RFC-3339 with an offset"))
}

fn require_identifier(value: &str) -> Result<(), HealthError> {
    require_text(value, 200)
}

fn require_text(value: &str, max: usize) -> Result<(), HealthError> {
    if value.trim().is_empty() || value.len() > max {
        Err(HealthError::Invalid("required text is blank or oversized"))
    } else {
        Ok(())
    }
}

/// Health payload failure.
#[derive(Debug, Error)]
pub enum HealthError {
    /// Request exceeded the receiver's fixed one-MiB boundary.
    #[error("Health payload exceeds 1 MiB")]
    PayloadTooLarge,
    /// JSON did not match the strict schema.
    #[error("invalid Health JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A semantic payload constraint failed.
    #[error("invalid Health payload: {0}")]
    Invalid(&'static str),
    /// Existing storage failed startup validation.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// Transactional persistence failed.
    #[error("Health import storage failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
