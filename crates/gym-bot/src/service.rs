//! Transport-neutral deterministic gym domain service.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::Arc,
};

use chrono::{DateTime, FixedOffset};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use thiserror::Error;

use crate::{
    batch::{BatchError, BatchService},
    clock::Clock,
    command::{WeightParseError, parse_weight_command},
    database::{DatabaseError, open_existing},
};

const GYM_USAGE: &str = "Usage: /gym <exercise> <sets>x<reps> [weight] [@rpe]";
const CARDIO_USAGE: &str = "Usage: /cardio <activity> <minutes> [distance_km]";
const RATE_USAGE: &str = "Usage: /rate <1-5> [notes]";
const PREFERENCE_USAGE: &str = "Usage: /preference <key> <value>";
const BATCH_USAGE: &str = "Usage: /batch [open|status|flush|cancel|retry]";
const ADHERENCE_USAGE: &str = "Usage: /adherence [number of plans]";
const AGENT_UNAVAILABLE: &str =
    "Agent-dependent gym behavior is unavailable while architecture decision D23 is unresolved.";
const BATCH_EXTRACTION_UNAVAILABLE: &str = "Batch extraction is agent-dependent and unavailable while architecture decision D23 is unresolved; the buffer was kept for /batch retry.";
const IMPORT_UNAVAILABLE: &str =
    "Apple Health archive import is unavailable through this deterministic service.";
const UNKNOWN_COMMAND: &str = "Unknown command. Use /help to see supported gym commands.";

/// Request accepted by the deterministic gym domain service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceRequest {
    /// Stable conversation identity, independent of transport.
    pub conversation_id: String,
    /// Plain user-authored text.
    pub text: String,
}

/// Owner decision for a previously proposed inferred preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreferenceReviewDecision {
    /// Activate the inferred preference.
    Keep,
    /// Leave the inferred preference inactive.
    Reject,
}

/// Transport-neutral review of one inferred preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreferenceReviewRequest {
    /// Frozen-schema preference row identity.
    pub preference_id: i64,
    /// Explicit owner decision.
    pub decision: PreferenceReviewDecision,
}

/// Deterministic gym command and query service.
pub struct GymService {
    database_path: PathBuf,
    clock: Arc<dyn Clock>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BatchCommand {
    Status,
    Cancel,
    Flush,
    Retry,
    Toggle,
    Open,
}

impl BatchCommand {
    const fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            None => Some(Self::Toggle),
            Some(value) if value.eq_ignore_ascii_case("status") => Some(Self::Status),
            Some(value) if value.eq_ignore_ascii_case("cancel") => Some(Self::Cancel),
            Some(value) if value.eq_ignore_ascii_case("flush") => Some(Self::Flush),
            Some(value) if value.eq_ignore_ascii_case("retry") => Some(Self::Retry),
            Some(value) if value.eq_ignore_ascii_case("toggle") => Some(Self::Toggle),
            Some(value) if value.eq_ignore_ascii_case("open") => Some(Self::Open),
            Some(_) => None,
        }
    }
}

impl GymService {
    /// Creates a service against the frozen v0 schema.
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        Self {
            database_path: database_path.into(),
            clock,
        }
    }

    /// Executes deterministic zero-token behavior.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] when storage or an injected timestamp fails.
    pub fn handle(&self, request: &ServiceRequest) -> Result<String, ServiceError> {
        let connection = open_existing(&self.database_path)?;
        let text = request.text.trim();
        Ok(match text.split_whitespace().next().unwrap_or_default() {
            "/weight" => self.weight(&connection, text)?,
            "/gym" => self.strength(&connection, text)?,
            "/cardio" | "/run" => self.cardio(&connection, text)?,
            "/batch" => self.batch(&connection, request, text)?,
            "/plans" => plans(&connection, text)?,
            "/plan" => plan(&connection, text)?,
            "/rate" => rate(&connection, text)?,
            "/adherence" => adherence(&connection, text)?,
            "/cost" => cost(&connection)?,
            "/sync" => sync_status(&connection)?,
            "/export" => self.export(&connection)?,
            "/import_zip" => IMPORT_UNAVAILABLE.to_owned(),
            "/preference" => preference(&connection, text)?,
            "/help" => help_text().to_owned(),
            command if command.starts_with('/') => UNKNOWN_COMMAND.to_owned(),
            _ => AGENT_UNAVAILABLE.to_owned(),
        })
    }

    /// Applies a deterministic Keep/Reject preference-review callback.
    ///
    /// The conditional update makes retries harmless: only an unreviewed,
    /// inactive inferred preference can transition once.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] when frozen storage is unavailable.
    pub fn review_preference(
        &self,
        request: PreferenceReviewRequest,
    ) -> Result<String, ServiceError> {
        let connection = open_existing(&self.database_path)?;
        let active = i64::from(request.decision == PreferenceReviewDecision::Keep);
        let updated = connection.execute(
            "UPDATE preferences SET active=?1, reviewed_at=?2 \
             WHERE id=?3 AND source='inferred' AND active=0 AND reviewed_at IS NULL",
            params![active, self.clock.now_iso8601(), request.preference_id],
        )?;
        Ok(if updated == 0 {
            "This preference was already reviewed.".to_owned()
        } else {
            match request.decision {
                PreferenceReviewDecision::Keep => "Preference accepted.".to_owned(),
                PreferenceReviewDecision::Reject => "Preference rejected.".to_owned(),
            }
        })
    }

    fn weight(&self, connection: &Connection, text: &str) -> Result<String, ServiceError> {
        if text.split_whitespace().count() == 1 {
            let mut statement = connection.prepare(
                "SELECT date, value FROM body_metrics WHERE metric='weight_kg' \
                 ORDER BY date DESC, id DESC LIMIT 20",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(if rows.is_empty() {
                "No weights logged yet.".to_owned()
            } else {
                rows.into_iter()
                    .map(|(date, value)| {
                        format!(
                            "{} · {} kg",
                            date.get(..10).unwrap_or(&date),
                            General(value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        }
        let parsed = match parse_weight_command(text) {
            Ok(parsed) => parsed,
            Err(WeightParseError::Usage) => return Ok("Usage: /weight <kg>".to_owned()),
        };
        connection.execute(
            "INSERT INTO body_metrics (date, metric, value, unit, source) \
             VALUES (?1, 'weight_kg', ?2, 'kg', 'manual')",
            params![self.clock.now_iso8601(), parsed.kilograms()],
        )?;
        Ok(format!("Logged weight: {} kg", General(parsed.kilograms())))
    }

    fn strength(&self, connection: &Connection, text: &str) -> Result<String, ServiceError> {
        let tokens = text.split_whitespace().skip(1).collect::<Vec<_>>();
        let Some(index) = tokens.iter().position(|token| set_spec(token).is_some()) else {
            return recent_strength(connection, &tokens.join(" "));
        };
        if index == 0 {
            return Ok(GYM_USAGE.to_owned());
        }
        let display = tokens[..index].join(" ");
        let name = canonical_name(&display);
        let (sets, reps) = set_spec(tokens[index]).expect("set spec located above");
        if !(1..=50).contains(&sets) || !(1..=1000).contains(&reps) {
            return Ok(GYM_USAGE.to_owned());
        }
        let mut weight = None;
        let mut rpe = None;
        for token in &tokens[index + 1..] {
            if let Some(value) = token.strip_prefix('@') {
                let parsed = value.parse::<u8>().ok();
                if parsed.is_none_or(|value| !(1..=10).contains(&value)) {
                    return Ok(GYM_USAGE.to_owned());
                }
                rpe = parsed;
            } else {
                let parsed = token
                    .strip_suffix("kg")
                    .unwrap_or(token)
                    .parse::<f64>()
                    .ok();
                if parsed.is_none_or(|value| !value.is_finite() || !(0.0..=1000.0).contains(&value))
                {
                    return Ok(GYM_USAGE.to_owned());
                }
                weight = parsed;
            }
        }
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO sessions (started_at, kind) VALUES (?1, 'strength')",
            [self.clock.now_iso8601()],
        )?;
        let session_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO movements (name, display_name, modality) VALUES (?1, ?1, 'strength') \
             ON CONFLICT(name) DO NOTHING",
            [&name],
        )?;
        let movement_id: i64 =
            transaction.query_row("SELECT id FROM movements WHERE name=?1", [&name], |row| {
                row.get(0)
            })?;
        transaction.execute(
            "INSERT INTO session_items (session_id, position, movement_id) VALUES (?1, 1, ?2)",
            params![session_id, movement_id],
        )?;
        let item_id = transaction.last_insert_rowid();
        for position in 1..=sets {
            transaction.execute(
                "INSERT INTO efforts (session_item_id, position, reps, weight_kg, rpe) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![item_id, position, reps, weight, rpe],
            )?;
        }
        transaction.commit()?;
        Ok(format!(
            "Logged {display} — {sets}x{reps}{}",
            weight
                .map(|value| format!(" @ {}kg", General(value)))
                .unwrap_or_default()
        ))
    }

    fn cardio(&self, connection: &Connection, text: &str) -> Result<String, ServiceError> {
        let tokens = text.split_whitespace().skip(1).collect::<Vec<_>>();
        if tokens.len() < 2 {
            return Ok(CARDIO_USAGE.to_owned());
        }
        let has_distance = tokens.len() >= 3;
        let duration_index = if has_distance {
            tokens.len() - 2
        } else {
            tokens.len() - 1
        };
        let display = tokens[..duration_index].join(" ");
        let duration = tokens[duration_index].parse::<f64>().ok();
        let distance = has_distance
            .then(|| tokens[tokens.len() - 1].parse::<f64>().ok())
            .flatten();
        let Some(duration) = duration.filter(|value| value.is_finite() && *value > 0.0) else {
            return Ok(CARDIO_USAGE.to_owned());
        };
        if display.is_empty()
            || has_distance && distance.is_none_or(|value| !value.is_finite() || value < 0.0)
        {
            return Ok(CARDIO_USAGE.to_owned());
        }
        let name = display.to_lowercase();
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO sessions (started_at, kind, source) VALUES (?1, 'cardio', 'manual')",
            [self.clock.now_iso8601()],
        )?;
        let session_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO movements (name, display_name, modality) VALUES (?1, ?2, 'cardio') \
             ON CONFLICT(name) DO NOTHING",
            params![name, display],
        )?;
        let movement_id: i64 =
            transaction.query_row("SELECT id FROM movements WHERE name=?1", [&name], |row| {
                row.get(0)
            })?;
        transaction.execute(
            "INSERT INTO session_items (session_id, position, movement_id) VALUES (?1, 1, ?2)",
            params![session_id, movement_id],
        )?;
        let item_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO efforts (session_item_id, position, duration_s, distance_m) \
             VALUES (?1, 1, ?2, ?3)",
            params![
                item_id,
                duration * 60.0,
                distance.map(|value| value * 1000.0)
            ],
        )?;
        transaction.commit()?;
        Ok(format!(
            "Logged {display}: {} min{}",
            General(duration),
            distance
                .map(|value| format!(", {} km", General(value)))
                .unwrap_or_default()
        ))
    }

    fn batch(
        &self,
        _connection: &Connection,
        request: &ServiceRequest,
        text: &str,
    ) -> Result<String, ServiceError> {
        let Ok(chat_id) = request.conversation_id.parse::<i64>() else {
            return Ok("Batch commands require a numeric conversation id.".to_owned());
        };
        let mut tokens = text.split_whitespace().skip(1);
        let subcommand = BatchCommand::parse(tokens.next());
        if tokens.next().is_some() {
            return Ok(BATCH_USAGE.to_owned());
        }
        let batch = BatchService::new(&self.database_path);
        match subcommand {
            Some(BatchCommand::Status) => {
                let (count, earliest) = batch.status(chat_id)?;
                Ok(format!(
                    "Batch: {count} messages{}",
                    earliest
                        .map(|value| format!(" since {}", batch_display_time(&value)))
                        .unwrap_or_default()
                ))
            }
            Some(BatchCommand::Cancel) => {
                let count = batch.cancel(chat_id)?;
                Ok(format!("Cancelled batch with {count} buffered messages."))
            }
            Some(BatchCommand::Flush | BatchCommand::Retry) => batch_flush_reply(&batch, chat_id),
            Some(BatchCommand::Toggle) => {
                let active = batch.active(chat_id)?;
                if active {
                    batch_flush_reply(&batch, chat_id)
                } else {
                    self.open_batch(&batch, chat_id)
                }
            }
            Some(BatchCommand::Open) => self.open_batch(&batch, chat_id),
            None => Ok(BATCH_USAGE.to_owned()),
        }
    }

    fn open_batch(&self, batch: &BatchService, chat_id: i64) -> Result<String, ServiceError> {
        let now = DateTime::<FixedOffset>::parse_from_rfc3339(&self.clock.now_iso8601())?;
        batch.open(chat_id, now)?;
        Ok(
            "Batch opened. Send entries as normal messages; use /batch again when ready."
                .to_owned(),
        )
    }

    fn export(&self, connection: &Connection) -> Result<String, ServiceError> {
        let destination = self
            .database_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("exports/efforts.csv");
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = BufWriter::new(File::create(&destination)?);
        output.write_all(
            b"started_at,movement,position,reps,weight_kg,duration_s,distance_m,rpe,notes\r\n",
        )?;
        let mut statement = connection.prepare(
            "SELECT s.started_at, m.name, e.position, e.reps, e.weight_kg, e.duration_s, \
             e.distance_m, e.rpe, e.notes FROM efforts e \
             JOIN session_items i ON i.id=e.session_item_id JOIN sessions s ON s.id=i.session_id \
             JOIN movements m ON m.id=i.movement_id ORDER BY s.started_at, e.position",
        )?;
        let mut rows = statement.query([])?;
        let mut count = 0_u64;
        while let Some(row) = rows.next()? {
            let values = [
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?.to_string(),
                optional_csv::<i64>(row.get(3)?),
                optional_csv_float(row.get(4)?),
                optional_csv_float(row.get(5)?),
                optional_csv_float(row.get(6)?),
                optional_csv::<i64>(row.get(7)?),
                row.get::<_, Option<String>>(8)?.unwrap_or_default(),
            ];
            write!(
                output,
                "{}\r\n",
                values.map(|value| csv_field(&value)).join(",")
            )?;
            count += 1;
        }
        output.flush()?;
        Ok(format!(
            "Exported {count} efforts to {}",
            destination.display()
        ))
    }
}

fn batch_flush_reply(batch: &BatchService, chat_id: i64) -> Result<String, ServiceError> {
    let (count, _) = batch.status(chat_id)?;
    Ok(if count == 0 {
        "No active batch.".to_owned()
    } else {
        BATCH_EXTRACTION_UNAVAILABLE.to_owned()
    })
}

fn batch_display_time(value: &str) -> String {
    value.get(..16).unwrap_or(value).replace('T', " ")
}

fn optional_csv<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn optional_csv_float(value: Option<f64>) -> String {
    value.map(|value| format!("{value:?}")).unwrap_or_default()
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn recent_strength(connection: &Connection, requested: &str) -> Result<String, ServiceError> {
    let requested = requested.trim();
    let name = (!requested.is_empty()).then(|| canonical_name(requested));
    let sql = if name.is_some() {
        "SELECT s.started_at, m.name, e.reps, e.weight_kg, e.rpe FROM efforts e \
         JOIN session_items i ON i.id=e.session_item_id JOIN sessions s ON s.id=i.session_id \
         JOIN movements m ON m.id=i.movement_id WHERE m.modality='strength' AND m.name=?1 \
         ORDER BY s.started_at DESC, e.position DESC LIMIT 10"
    } else {
        "SELECT s.started_at, m.name, e.reps, e.weight_kg, e.rpe FROM efforts e \
         JOIN session_items i ON i.id=e.session_item_id JOIN sessions s ON s.id=i.session_id \
         JOIN movements m ON m.id=i.movement_id WHERE m.modality='strength' \
         ORDER BY s.started_at DESC, e.position DESC LIMIT 10"
    };
    let mut statement = connection.prepare(sql)?;
    let mut rows = if let Some(name) = name {
        statement.query([name])?
    } else {
        statement.query([])?
    };
    let mut lines = Vec::new();
    while let Some(row) = rows.next()? {
        let date: String = row.get(0)?;
        let exercise: String = row.get(1)?;
        let reps: i64 = row.get(2)?;
        let weight: Option<f64> = row.get(3)?;
        let rpe: Option<i64> = row.get(4)?;
        lines.push(format!(
            "{} · {exercise} {reps} reps{}{}",
            date.get(..10).unwrap_or(&date),
            weight
                .map(|value| format!(" @ {}kg", General(value)))
                .unwrap_or_default(),
            rpe.map(|value| format!(" RPE {value}")).unwrap_or_default()
        ));
    }
    if lines.is_empty() {
        let label = if requested.is_empty() {
            "strength workouts"
        } else {
            requested
        };
        Ok(format!("No {label} logged yet. {GYM_USAGE}"))
    } else {
        Ok(format!(
            "Recent strength{}:\n{}",
            if requested.is_empty() {
                String::new()
            } else {
                format!(" — {requested}")
            },
            lines.join("\n")
        ))
    }
}

fn plans(connection: &Connection, text: &str) -> Result<String, ServiceError> {
    let limit = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(10)
        .clamp(1, 100);
    let mut statement = connection.prepare(
        "SELECT id, created_at, focus, status, rating FROM workout_plans ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = statement
        .query_map([limit], |row| {
            let created: String = row.get(1)?;
            Ok(format!(
                "#{} · {} · {} · {}{}",
                row.get::<_, i64>(0)?,
                created.get(..10).unwrap_or(&created),
                row.get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "unspecified".to_owned()),
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?
                    .map(|value| format!(" · rated {value}"))
                    .unwrap_or_default()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if rows.is_empty() {
        "No plans yet.".to_owned()
    } else {
        format!("{}\n\nUse /plan <id> to view one in full.", rows.join("\n"))
    })
}

fn plan(connection: &Connection, text: &str) -> Result<String, ServiceError> {
    let Some(id) = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return Ok(AGENT_UNAVAILABLE.to_owned());
    };
    let row = connection
        .query_row(
            "SELECT focus, plan_json, status FROM workout_plans WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map_or_else(
        || format!("No plan #{id}."),
        |(focus, value, status)| {
            format!(
                "Plan #{id} · {} · {status}\n{value}",
                focus.unwrap_or_else(|| "unspecified".to_owned())
            )
        },
    ))
}

fn rate(connection: &Connection, text: &str) -> Result<String, ServiceError> {
    let mut tokens = text.split_whitespace().skip(1);
    let Some(rating) = tokens
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| (1..=5).contains(value))
    else {
        return Ok(RATE_USAGE.to_owned());
    };
    let feedback = tokens.collect::<Vec<_>>().join(" ");
    let latest = connection
        .query_row(
            "SELECT id FROM workout_plans ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(latest) = latest else {
        return Ok("No plan to rate yet.".to_owned());
    };
    connection.execute(
        "UPDATE workout_plans SET rating=?1, feedback=?2, status='completed' WHERE id=?3",
        params![rating, (!feedback.is_empty()).then_some(feedback), latest],
    )?;
    Ok("Thanks — plan feedback saved.".to_owned())
}

fn adherence(connection: &Connection, text: &str) -> Result<String, ServiceError> {
    let mut tokens = text.split_whitespace().skip(1);
    let limit = match tokens.next() {
        Some(value) => match value.parse::<u16>() {
            Ok(value) => value.clamp(1, 20),
            Err(_) => return Ok(ADHERENCE_USAGE.to_owned()),
        },
        None => 5,
    };
    if tokens.next().is_some() {
        return Ok(ADHERENCE_USAGE.to_owned());
    }
    let mut statement = connection.prepare(
        "SELECT id, coalesce(for_date, substr(created_at,1,10)), plan_json \
         FROM workout_plans ORDER BY id DESC LIMIT ?1",
    )?;
    let plans = statement
        .query_map([limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if plans.is_empty() {
        return Ok("No plans to compare yet.".to_owned());
    }
    let mut lines = Vec::with_capacity(plans.len());
    for (plan_id, date, plan_json) in plans {
        let value: Value = serde_json::from_str(&plan_json)?;
        let items = value
            .get("items")
            .and_then(Value::as_array)
            .filter(|items| (1..=20).contains(&items.len()))
            .ok_or(ServiceError::InvalidPlanJson)?;
        let exercises = items
            .iter()
            .map(|item| {
                item.get("exercise")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|exercise| !exercise.is_empty())
                    .map(str::to_lowercase)
                    .ok_or(ServiceError::InvalidPlanJson)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut actual_statement = connection.prepare(
            "SELECT DISTINCT m.name FROM sessions s \
             JOIN session_items i ON i.session_id=s.id \
             JOIN movements m ON m.id=i.movement_id \
             WHERE substr(s.started_at,1,10)=?1 AND m.modality='strength'",
        )?;
        let actual = actual_statement
            .query_map([&date], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        let completed = exercises
            .iter()
            .filter(|exercise| actual.contains(*exercise))
            .count();
        lines.push(format!(
            "#{plan_id} · {date} · {completed}/{} exercises",
            exercises.len()
        ));
    }
    Ok(lines.join("\n"))
}

fn cost(connection: &Connection) -> Result<String, ServiceError> {
    let (calls, prompt, completion): (i64, i64, i64) = connection.query_row(
        "SELECT count(*), coalesce(sum(prompt_tokens),0), coalesce(sum(completion_tokens),0) \
         FROM model_calls WHERE ok=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(format!(
        "Model usage: {calls} calls · {prompt} input · {completion} output tokens"
    ))
}

fn sync_status(connection: &Connection) -> Result<String, ServiceError> {
    let (count, latest): (i64, Option<String>) = connection.query_row(
        "SELECT count(*), max(imported_at) FROM external_activities WHERE source='apple_health'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(format!(
        "Apple Health: {count} records; last import {}",
        latest.unwrap_or_else(|| "never".to_owned())
    ))
}

fn preference(connection: &Connection, text: &str) -> Result<String, ServiceError> {
    let mut tokens = text.split_whitespace().skip(1);
    let Some(key) = tokens.next() else {
        return Ok(PREFERENCE_USAGE.to_owned());
    };
    let value = tokens.collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        return Ok(PREFERENCE_USAGE.to_owned());
    }
    connection.execute(
        "INSERT INTO preferences (key, value, confidence, source, active, reviewed_at) \
         VALUES (?1, ?2, 1.0, 'stated', 1, CURRENT_TIMESTAMP)",
        params![key, value],
    )?;
    Ok("Preference saved.".to_owned())
}

fn set_spec(token: &str) -> Option<(u16, u16)> {
    let lowercase = token.to_ascii_lowercase();
    let (sets, reps) = lowercase.split_once('x')?;
    Some((sets.parse().ok()?, reps.parse().ok()?))
}

fn canonical_name(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .map(|word| match word {
            "db" | "dumbell" | "dumbells" => "dumbbell".to_owned(),
            "bb" => "barbell".to_owned(),
            "kb" | "kettlebells" => "kettlebell".to_owned(),
            "ohp" => "overhead press".to_owned(),
            "rdl" => "romanian deadlift".to_owned(),
            "dl" | "deads" => "deadlift".to_owned(),
            "flys" | "flyes" => "fly".to_owned(),
            "calves" => "calf".to_owned(),
            _ if word.ends_with("ies") && word.len() > 4 => {
                format!("{}y", &word[..word.len() - 3])
            }
            _ if word.ends_with('s') && !word.ends_with("ss") && word.len() > 2 => {
                word[..word.len() - 1].to_owned()
            }
            _ => word.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const fn help_text() -> &'static str {
    "/gym <exercise> <sets>x<reps> [weight]kg — log strength (no args: recent history)\n/cardio <activity> <minutes> [distance_km] — log cardio\n/run — alias for /cardio\n/weight [kg] — log body weight (no args: recent history)\n/batch [open|status|flush|cancel|retry] — manage buffered logging; extraction is unavailable under D23\n/plan <id> — show a stored plan; generation is unavailable under D23\n/plans [n] — list recent plans\n/rate <1-5> [notes] — rate the latest plan\n/adherence [n] — compare stored plans with logged strength\n/cost — show stored model usage\n/sync — show Apple Health import status\n/export — export logged efforts to CSV\n/import_zip — unavailable through this deterministic service\n/preference <key> <value> — record a stated preference\n/help — show this message"
}

struct General(f64);

impl std::fmt::Display for General {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Deterministic gym service failure.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Existing frozen storage is unavailable or incompatible.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// A deterministic read or write failed.
    #[error("gym service storage failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Durable batch storage failed.
    #[error(transparent)]
    Batch(#[from] BatchError),
    /// The injected clock did not return the documented RFC-3339 shape.
    #[error("gym service clock returned an invalid timestamp: {0}")]
    Time(#[from] chrono::ParseError),
    /// A stored plan violated the frozen validated plan JSON contract.
    #[error("stored workout plan JSON is invalid")]
    InvalidPlanJson,
    /// Stored workout plan JSON could not be decoded.
    #[error("stored workout plan JSON could not be decoded: {0}")]
    Json(#[from] serde_json::Error),
    /// A deterministic export could not be written.
    #[error("gym export failed: {0}")]
    Io(#[from] std::io::Error),
}
