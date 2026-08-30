//! Transport-neutral deterministic gym domain service.

use std::{path::PathBuf, sync::Arc};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::{
    clock::Clock,
    command::{WeightParseError, parse_weight_command},
    database::{DatabaseError, open_existing},
};

const GYM_USAGE: &str = "Usage: /gym <exercise> <sets>x<reps> [weight] [@rpe]";
const CARDIO_USAGE: &str = "Usage: /cardio <activity> <minutes> [distance_km]";
const RATE_USAGE: &str = "Usage: /rate <1-5> [notes]";
const PREFERENCE_USAGE: &str = "Usage: /preference <key> <value>";
const AGENT_UNAVAILABLE: &str =
    "Agent-dependent gym behavior is unavailable while architecture decision D23 is unresolved.";

/// Request accepted by the deterministic gym domain service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceRequest {
    /// Stable conversation identity, independent of transport.
    pub conversation_id: String,
    /// Plain user-authored text.
    pub text: String,
}

/// Deterministic gym command and query service.
pub struct GymService {
    database_path: PathBuf,
    clock: Arc<dyn Clock>,
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
            "/plans" => plans(&connection, text)?,
            "/plan" => plan(&connection, text)?,
            "/rate" => rate(&connection, text)?,
            "/cost" => cost(&connection)?,
            "/sync" => sync_status(&connection)?,
            "/preference" => preference(&connection, text)?,
            "/help" => help_text().to_owned(),
            _ => AGENT_UNAVAILABLE.to_owned(),
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
            params![self.clock.now_iso8601(), parsed.kilograms],
        )?;
        Ok(format!("Logged weight: {} kg", General(parsed.kilograms)))
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
    "/gym <exercise> <sets>x<reps> [weight]kg — log strength (no args: recent history)\n/cardio <activity> <minutes> [distance_km] — log cardio\n/run — alias for /cardio\n/weight [kg] — log body weight (no args: recent history)\n/plan <id> — show a stored plan; generation is unavailable under D23\n/plans [n] — list recent plans\n/rate <1-5> [notes] — rate the latest plan\n/cost — show stored model usage\n/sync — show Apple Health import status\n/preference <key> <value> — record a stated preference\n/help — show this message"
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
}
