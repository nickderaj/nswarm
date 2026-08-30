//! Bounded read-only gym MCP surface over a Unix domain socket.

use std::{
    collections::HashMap,
    future::Future,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(target_os = "linux")]
use std::os::linux::fs::MetadataExt as LinuxMetadataExt;

use chrono::{DateTime, Days};
use rmcp::{
    RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, ErrorData,
        Implementation, JsonObject, ListToolsResult, ServerCapabilities, ServerInfo, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
};
use rusqlite::{Connection, OptionalExtension, params, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::net::{UnixListener, UnixStream};

use crate::{
    clock::Clock,
    database::{DatabaseError, open_existing, open_existing_read_only, validate_existing},
};

/// Default v0-compatible lookback in days.
pub const DEFAULT_DAYS: u16 = 56;
/// Maximum accepted lookback in days.
pub const MAX_DAYS: u16 = 365;
/// Default number of returned body-metric rows.
pub const DEFAULT_LIMIT: u16 = 200;
/// Maximum accepted number of returned body-metric rows.
pub const MAX_LIMIT: u16 = 200;
/// Maximum accepted metric-name length.
pub const MAX_METRIC_LENGTH: usize = 64;
/// Exact reviewed v0 gym MCP allow-list.
pub const GYM_TOOL_NAMES: [&str; 13] = [
    "recent_sets",
    "exercise_catalogue",
    "volume_summary",
    "body_metrics",
    "recent_runs",
    "pace_trend",
    "interval_history",
    "heart_rate_series",
    "weekly_load",
    "preferences",
    "record_preference",
    "propose_plan",
    "plan_feedback",
];

/// Unix byte stream used by the rmcp client/server transport.
pub type McpSocketStream = UnixStream;

/// Validated arguments for the only exposed gym MCP tool.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BodyMetricsArgs {
    /// Optional exact metric name, such as `weight_kg`.
    pub metric: Option<String>,
    /// Inclusive lookback in days; defaults to 56 and is capped at 365.
    pub days: Option<u16>,
    /// Maximum rows; defaults to and is capped at 200.
    pub limit: Option<u16>,
}

impl BodyMetricsArgs {
    /// Validates and fills defaults for the bounded query.
    ///
    /// # Errors
    ///
    /// Returns [`McpQueryError`] for blank/oversized metric names or zero and
    /// over-limit numeric bounds.
    pub fn validate(self) -> Result<ValidatedBodyMetricsArgs, McpQueryError> {
        let metric = self
            .metric
            .map(|metric| validate_metric(&metric))
            .transpose()?;
        let days = self.days.unwrap_or(DEFAULT_DAYS);
        if !(1..=MAX_DAYS).contains(&days) {
            return Err(McpQueryError::DaysOutOfRange(days));
        }
        let limit = self.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(McpQueryError::LimitOutOfRange(limit));
        }
        Ok(ValidatedBodyMetricsArgs {
            metric,
            days,
            limit,
        })
    }
}

fn validate_metric(metric: &str) -> Result<String, McpQueryError> {
    if metric.is_empty()
        || metric.len() > MAX_METRIC_LENGTH
        || !metric
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(McpQueryError::InvalidMetric);
    }
    Ok(metric.to_owned())
}

/// Fully bounded body-metrics query parameters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedBodyMetricsArgs {
    /// Optional exact metric filter.
    pub metric: Option<String>,
    /// Validated inclusive lookback.
    pub days: u16,
    /// Validated row cap.
    pub limit: u16,
}

/// One read-only body metric returned by MCP.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BodyMetric {
    /// ISO-8601 timestamp stored by the v0 schema.
    pub date: String,
    /// Metric name.
    pub metric: String,
    /// Numeric metric value.
    pub value: f64,
    /// Unit label.
    pub unit: String,
    /// Import or manual source label.
    pub source: String,
}

/// Production implementation of the bounded gym tool.
#[derive(Clone)]
pub struct GymMcp {
    database_path: PathBuf,
    clock: Arc<dyn Clock>,
}

impl GymMcp {
    /// Creates the production MCP handler against an existing gym database.
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        Self {
            database_path: database_path.into(),
            clock,
        }
    }

    /// Executes the same bounded read used by the MCP handler.
    ///
    /// # Errors
    ///
    /// Returns [`McpQueryError`] when bounds, database validation, or the fixed
    /// read query fails. Every row in the indexed candidate window must contain
    /// an RFC-3339 timestamp with an explicit offset; one invalid timestamp
    /// fails the whole call rather than returning silent partial results.
    pub fn body_metrics(&self, args: BodyMetricsArgs) -> Result<Vec<BodyMetric>, McpQueryError> {
        let args = args.validate()?;
        let connection = open_existing_read_only(&self.database_path)?;
        let now = DateTime::parse_from_rfc3339(&self.clock.now_iso8601())?;
        let cutoff = now
            .checked_sub_days(Days::new(u64::from(args.days)))
            .ok_or(McpQueryError::ClockOutOfRange)?;
        // RFC-3339 offsets can move an instant's textual calendar date by one
        // day. Start two days earlier so this raw predicate is only an indexed
        // prefilter; parsed instants below decide inclusion and ordering.
        let index_floor = cutoff
            .checked_sub_days(Days::new(2))
            .ok_or(McpQueryError::ClockOutOfRange)?
            .format("%Y-%m-%d")
            .to_string();
        let mut candidates = Vec::new();
        if let Some(metric) = args.metric {
            let mut statement = connection.prepare(
                "SELECT id, date, metric, value, unit, source FROM body_metrics \
                 WHERE metric = ?1 AND date >= ?2 \
                 ORDER BY date DESC, id DESC",
            )?;
            let mapped = statement.query_map(params![metric, index_floor], metric_from_row)?;
            for row in mapped {
                candidates.push(row?);
            }
        } else {
            let mut statement = connection.prepare(
                "SELECT id, date, metric, value, unit, source FROM body_metrics \
                 WHERE date >= ?1 \
                 ORDER BY date DESC, id DESC",
            )?;
            let mapped = statement.query_map(params![index_floor], metric_from_row)?;
            for row in mapped {
                candidates.push(row?);
            }
        }
        let mut rows = candidates
            .into_iter()
            .map(|(id, metric)| {
                let at = DateTime::parse_from_rfc3339(&metric.date)
                    .map_err(|_| McpQueryError::StoredTimestamp(metric.date.clone()))?;
                Ok((at, id, metric))
            })
            .collect::<Result<Vec<_>, McpQueryError>>()?;
        rows.retain(|(at, _, _)| *at >= cutoff);
        rows.sort_by(|left, right| right.0.cmp(&left.0).then(right.1.cmp(&left.1)));
        rows.truncate(usize::from(args.limit));
        Ok(rows.into_iter().map(|(_, _, metric)| metric).collect())
    }
}

fn metric_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, BodyMetric)> {
    Ok((
        row.get(0)?,
        BodyMetric {
            date: row.get(1)?,
            metric: row.get(2)?,
            value: row.get(3)?,
            unit: row.get(4)?,
            source: row.get(5)?,
        },
    ))
}

impl ServerHandler for GymMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("nswarm-gym", env!("CARGO_PKG_VERSION")))
            .with_instructions("Reviewed gym tools only; no resources, prompts, sampling, filesystem, shell, network, or raw SQL")
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(gym_tools())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !GYM_TOOL_NAMES.contains(&request.name.as_ref()) {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "unknown gym tool",
                None,
            ));
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        if request.name == "body_metrics" {
            let args: BodyMetricsArgs = serde_json::from_value(arguments)
                .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
            let rows = self.body_metrics(args).map_err(|error| match error {
                McpQueryError::InvalidMetric
                | McpQueryError::DaysOutOfRange(_)
                | McpQueryError::LimitOutOfRange(_) => {
                    ErrorData::invalid_params(error.to_string(), None)
                }
                McpQueryError::Database(_)
                | McpQueryError::Sqlite(_)
                | McpQueryError::StoredTimestamp(_) => {
                    ErrorData::internal_error("body_metrics storage is unavailable", None)
                }
                McpQueryError::ClockTimestamp(_) | McpQueryError::ClockOutOfRange => {
                    ErrorData::internal_error("body_metrics clock is invalid", None)
                }
            })?;
            let structured = serde_json::to_value(&rows)
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
            return Ok(CallToolResult::structured(structured).into());
        }
        let structured = self
            .call_reviewed_tool(&request.name, &arguments)
            .map_err(|error| match error {
                McpToolError::Invalid(message) => ErrorData::invalid_params(message, None),
                McpToolError::Storage(_) => {
                    ErrorData::internal_error("gym tool storage is unavailable", None)
                }
            })?;
        Ok(CallToolResult::structured(structured).into())
    }
}

impl GymMcp {
    fn call_reviewed_tool(&self, name: &str, arguments: &Value) -> Result<Value, McpToolError> {
        let connection = open_existing_read_only(&self.database_path)?;
        let object = arguments
            .as_object()
            .ok_or_else(|| McpToolError::Invalid("arguments must be an object".to_owned()))?;
        match name {
            "recent_sets" => query_json(
                &connection,
                "SELECT s.started_at, m.name AS exercise, e.reps, e.weight_kg, e.rpe \
                 FROM efforts e JOIN session_items i ON i.id=e.session_item_id \
                 JOIN sessions s ON s.id=i.session_id JOIN movements m ON m.id=i.movement_id \
                 WHERE m.modality='strength' ORDER BY s.started_at DESC, e.position DESC LIMIT ?1",
                [rusqlite::types::Value::Integer(bounded(object, "limit", 200, 1, 200)?)],
            ),
            "exercise_catalogue" => no_arguments(object).and_then(|()| query_json_no_params(&connection,
                "SELECT m.name AS exercise, m.muscle_groups, m.equipment, max(s.started_at) AS last_done, \
                 max(e.weight_kg) AS best_weight_kg, max(e.reps) AS best_reps FROM movements m \
                 LEFT JOIN session_items i ON i.movement_id=m.id LEFT JOIN sessions s ON s.id=i.session_id \
                 LEFT JOIN efforts e ON e.session_item_id=i.id WHERE m.modality='strength' \
                 GROUP BY m.id ORDER BY last_done DESC, m.name")),
            "volume_summary" => query_json(&connection,
                "SELECT strftime('%Y-%W', s.started_at) AS week, coalesce(m.muscle_groups, 'unclassified') AS muscle_groups, \
                 count(e.id) AS hard_sets, sum(coalesce(e.reps,0)*coalesce(e.weight_kg,0)) AS tonnage, \
                 count(DISTINCT s.id) AS sessions FROM efforts e JOIN session_items i ON i.id=e.session_item_id \
                 JOIN sessions s ON s.id=i.session_id JOIN movements m ON m.id=i.movement_id \
                 WHERE m.modality='strength' GROUP BY week, muscle_groups ORDER BY week DESC, muscle_groups LIMIT ?1",
                [rusqlite::types::Value::Integer(bounded(object, "weeks", 8, 1, 52)? * 53)]),
            "recent_runs" => query_json(&connection,
                "SELECT s.started_at, s.source, m.name AS activity, e.duration_s, e.distance_m, e.avg_hr, \
                 CASE WHEN e.distance_m > 0 THEN e.duration_s/(e.distance_m/1000.0) END AS seconds_per_km \
                 FROM efforts e JOIN session_items i ON i.id=e.session_item_id JOIN sessions s ON s.id=i.session_id \
                 JOIN movements m ON m.id=i.movement_id WHERE m.modality='cardio' ORDER BY s.started_at DESC LIMIT 200", []),
            "pace_trend" => query_json_no_params(&connection,
                "SELECT s.started_at, m.name AS activity, e.distance_m, e.duration_s, e.avg_hr, \
                 e.duration_s/(e.distance_m/1000.0) AS seconds_per_km FROM efforts e \
                 JOIN session_items i ON i.id=e.session_item_id JOIN sessions s ON s.id=i.session_id \
                 JOIN movements m ON m.id=i.movement_id WHERE m.modality='cardio' AND e.distance_m > 0 \
                 ORDER BY s.started_at DESC LIMIT 200"),
            "interval_history" => query_json(&connection,
                "SELECT s.started_at, m.name AS activity, x.position, x.distance_m, x.duration_s, x.avg_hr \
                 FROM effort_splits x JOIN session_items i ON i.id=x.session_item_id JOIN sessions s ON s.id=i.session_id \
                 JOIN movements m ON m.id=i.movement_id ORDER BY s.started_at DESC, x.position LIMIT ?1",
                [rusqlite::types::Value::Integer(bounded(object, "limit", 100, 1, 200)?)]),
            "heart_rate_series" => query_json(&connection,
                "SELECT s.started_at, m.name AS activity, h.at, h.bpm FROM hr_samples h \
                 JOIN session_items i ON i.id=h.session_item_id JOIN sessions s ON s.id=i.session_id \
                 JOIN movements m ON m.id=i.movement_id ORDER BY s.started_at DESC, h.at LIMIT ?1",
                [rusqlite::types::Value::Integer(bounded(object, "samples", 5000, 1, 5000)?)]),
            "weekly_load" => query_json_no_params(&connection,
                "SELECT substr(s.started_at,1,10) AS day, count(DISTINCT s.id) AS sessions, \
                 sum(coalesce(e.reps,0)*coalesce(e.weight_kg,0)) AS tonnage, sum(coalesce(e.distance_m,0)) AS distance_m \
                 FROM efforts e JOIN session_items i ON i.id=e.session_item_id JOIN sessions s ON s.id=i.session_id \
                 GROUP BY day ORDER BY day DESC LIMIT 364"),
            "preferences" => no_arguments(object).and_then(|()| query_json_no_params(&connection,
                "SELECT key, value, confidence, source, evidence FROM preferences WHERE active=1 ORDER BY updated_at DESC LIMIT 200")),
            "record_preference" => record_preference(&self.database_path, object),
            "propose_plan" => propose_plan(&self.database_path, object),
            "plan_feedback" => plan_feedback(&connection, object),
            _ => Err(McpToolError::Invalid("unknown gym tool".to_owned())),
        }
    }
}

fn gym_tools() -> Vec<Tool> {
    GYM_TOOL_NAMES
        .into_iter()
        .map(|name| {
            let write = matches!(name, "record_preference" | "propose_plan");
            Tool::new(
                name,
                format!("Reviewed gym {name} operation"),
                generic_schema(name),
            )
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(!write)
                    .destructive(false)
                    .idempotent(!write)
                    .open_world(false),
            )
        })
        .collect()
}

fn generic_schema(name: &str) -> JsonObject {
    let properties = match name {
        "body_metrics" => {
            json!({"metric":{"type":"string","minLength":1,"maxLength":MAX_METRIC_LENGTH,"pattern":"^[a-z0-9_]+$"},"days":{"type":"integer","minimum":1,"maximum":MAX_DAYS},"limit":{"type":"integer","minimum":1,"maximum":MAX_LIMIT}})
        }
        "record_preference" => {
            json!({"key":{"type":"string","minLength":1,"maxLength":80},"value":{"type":"string","minLength":1,"maxLength":500},"evidence":{"type":"string","minLength":1,"maxLength":1000}})
        }
        "propose_plan" => {
            json!({"focus":{"type":"string","minLength":1,"maxLength":200},"rationale":{"type":"string","minLength":1,"maxLength":2000},"items":{"type":"array","minItems":1,"maxItems":20}})
        }
        "plan_feedback" => json!({"plan_id":{"type":"integer","minimum":1}}),
        _ => json!({}),
    };
    serde_json::from_value(
        json!({"type":"object","additionalProperties":false,"properties":properties}),
    )
    .expect("static schema")
}

fn no_arguments(object: &serde_json::Map<String, Value>) -> Result<(), McpToolError> {
    if object.is_empty() {
        Ok(())
    } else {
        Err(McpToolError::Invalid(
            "tool accepts no arguments".to_owned(),
        ))
    }
}

fn bounded(
    object: &serde_json::Map<String, Value>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, McpToolError> {
    let value = object.get(key).map_or(Ok(default), |value| {
        value
            .as_i64()
            .ok_or_else(|| McpToolError::Invalid(format!("{key} must be an integer")))
    })?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(McpToolError::Invalid(format!(
            "{key} must be between {min} and {max}"
        )))
    }
}

fn query_json_no_params(connection: &Connection, sql: &str) -> Result<Value, McpToolError> {
    query_json(connection, sql, [])
}

fn query_json<const N: usize>(
    connection: &Connection,
    sql: &str,
    params: [rusqlite::types::Value; N],
) -> Result<Value, McpToolError> {
    let mut statement = connection.prepare(sql)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let rows = statement
        .query_map(rusqlite::params_from_iter(params), |row| {
            let mut object = serde_json::Map::new();
            for (index, name) in names.iter().enumerate() {
                let value = match row.get_ref(index)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(value) => json!(value),
                    ValueRef::Real(value) => json!(value),
                    ValueRef::Text(value) => {
                        Value::String(String::from_utf8_lossy(value).into_owned())
                    }
                    ValueRef::Blob(_) => Value::String("<blob>".to_owned()),
                };
                object.insert(name.clone(), value);
            }
            Ok(Value::Object(object))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(rows))
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<String, McpToolError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= max)
        .ok_or_else(|| McpToolError::Invalid(format!("{key} must be 1..={max} characters")))?;
    Ok(value.to_owned())
}

fn record_preference(
    path: &Path,
    object: &serde_json::Map<String, Value>,
) -> Result<Value, McpToolError> {
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "key" | "value" | "evidence"))
    {
        return Err(McpToolError::Invalid("unknown preference field".to_owned()));
    }
    let connection = open_existing(path)?;
    connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active,reviewed_at) VALUES (?1,?2,1.0,'stated',?3,1,CURRENT_TIMESTAMP)", params![required_string(object,"key",80)?,required_string(object,"value",500)?,required_string(object,"evidence",1000)?])?;
    Ok(json!({"id":connection.last_insert_rowid()}))
}

fn propose_plan(
    path: &Path,
    object: &serde_json::Map<String, Value>,
) -> Result<Value, McpToolError> {
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "focus" | "rationale" | "items" | "for_date"))
    {
        return Err(McpToolError::Invalid("unknown plan field".to_owned()));
    }
    let focus = required_string(object, "focus", 200)?;
    let rationale = required_string(object, "rationale", 2000)?;
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty() && items.len() <= 20)
        .ok_or_else(|| McpToolError::Invalid("items must contain 1..=20 entries".to_owned()))?;
    let plan = json!({"focus":focus,"rationale":rationale,"items":items});
    let connection = open_existing(path)?;
    connection.execute(
        "INSERT INTO workout_plans (for_date,focus,plan_json,rationale) VALUES (?1,?2,?3,?4)",
        params![
            object.get("for_date").and_then(Value::as_str),
            focus,
            plan.to_string(),
            rationale
        ],
    )?;
    Ok(json!({"id":connection.last_insert_rowid(),"plan":plan}))
}

fn plan_feedback(
    connection: &Connection,
    object: &serde_json::Map<String, Value>,
) -> Result<Value, McpToolError> {
    if object.keys().any(|key| key != "plan_id") {
        return Err(McpToolError::Invalid("unknown feedback field".to_owned()));
    }
    let id = bounded(object, "plan_id", 0, 1, i64::MAX)?;
    connection.query_row("SELECT status,rating,feedback FROM workout_plans WHERE id=?1", [id], |row| Ok(json!({"id":id,"status":row.get::<_,String>(0)?,"rating":row.get::<_,Option<i64>>(1)?,"feedback":row.get::<_,Option<String>>(2)?}))).optional()?.ok_or_else(|| McpToolError::Invalid(format!("unknown plan: {id}")))
}

#[derive(Debug, Error)]
enum McpToolError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Storage(#[from] DatabaseError),
}

impl From<rusqlite::Error> for McpToolError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(DatabaseError::Sqlite(error))
    }
}

/// Runs the production MCP server until its task is cancelled or an accept
/// operation fails. Every accepted stream performs a real MCP handshake.
///
/// The socket path is removed when this future is dropped or returns.
///
/// # Errors
///
/// Returns [`McpSocketError`] when the socket cannot be bound or accepted.
pub async fn run_mcp_server(
    socket_path: impl AsRef<Path>,
    socket_group: &str,
    database_path: impl Into<PathBuf>,
    clock: Arc<dyn Clock>,
) -> Result<(), McpSocketError> {
    McpServer::bind_for_group(socket_path, socket_group, database_path, clock)?
        .run()
        .await
}

/// A bound production MCP Unix-socket server.
pub struct McpServer {
    listener: UnixListener,
    _guard: SocketGuard,
    database_path: PathBuf,
    clock: Arc<dyn Clock>,
}

impl McpServer {
    /// Binds the socket synchronously with construction, making readiness and
    /// cleanup explicit to service managers and tests.
    ///
    /// # Errors
    ///
    /// Returns [`McpSocketError`] when the socket path cannot be bound.
    pub fn bind(
        socket_path: impl AsRef<Path>,
        database_path: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, McpSocketError> {
        Self::bind_inner(socket_path.as_ref(), None, database_path.into(), clock)
    }

    /// Binds the production socket and verifies the Fleet-declared group on Linux.
    ///
    /// # Errors
    ///
    /// Returns [`McpSocketError`] if the parent is not the expected setgid
    /// directory or the created socket does not inherit the expected group.
    pub fn bind_for_group(
        socket_path: impl AsRef<Path>,
        socket_group: &str,
        database_path: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, McpSocketError> {
        Self::bind_inner(
            socket_path.as_ref(),
            Some(socket_group),
            database_path.into(),
            clock,
        )
    }

    fn bind_inner(
        socket_path: &Path,
        socket_group: Option<&str>,
        database_path: PathBuf,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, McpSocketError> {
        let socket_path = socket_path.to_path_buf();
        validate_existing(&database_path)?;
        ensure_group_socket_parent(&socket_path, socket_group)?;
        let listener = UnixListener::bind(&socket_path).map_err(|error| {
            std::io::Error::new(error.kind(), format!("bind Unix socket: {error}"))
        })?;
        let guard = SocketGuard(socket_path.clone());
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660)).map_err(
            |error| std::io::Error::new(error.kind(), format!("set socket mode: {error}")),
        )?;
        #[cfg(target_os = "linux")]
        verify_expected_group(&socket_path, socket_group)?;
        Ok(Self {
            listener,
            _guard: guard,
            database_path,
            clock,
        })
    }

    /// Accepts MCP connections until cancelled or an accept fails.
    ///
    /// # Errors
    ///
    /// Returns [`McpSocketError`] when socket accept fails.
    pub async fn run(self) -> Result<(), McpSocketError> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let handler = GymMcp::new(self.database_path.clone(), Arc::clone(&self.clock));
            std::mem::drop(tokio::spawn(async move {
                match handler.serve(stream).await {
                    Ok(service) => {
                        if let Err(error) = service.waiting().await {
                            eprintln!("gym MCP connection failed: {error}");
                        }
                    }
                    Err(error) => eprintln!("gym MCP handshake failed: {error}"),
                }
            }));
        }
    }
}

fn ensure_group_socket_parent(
    socket_path: &Path,
    expected_group: Option<&str>,
) -> Result<(), std::io::Error> {
    #[cfg(not(target_os = "linux"))]
    let _ = expected_group;
    let parent = socket_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "gym MCP socket needs a parent directory",
        )
    })?;
    let metadata = std::fs::metadata(parent)?;
    let mode = metadata.permissions().mode();
    if mode & 0o777 != 0o750 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "gym MCP socket directory mode {:o} must be 750",
                mode & 0o777
            ),
        ));
    }
    #[cfg(target_os = "linux")]
    if let Some(group) = expected_group {
        if mode & 0o2000 == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "gym MCP socket directory must have setgid enabled",
            ));
        }
        verify_linux_group(&metadata, group, "directory")?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_expected_group(path: &Path, expected_group: Option<&str>) -> Result<(), std::io::Error> {
    if let Some(group) = expected_group {
        verify_linux_group(&std::fs::metadata(path)?, group, "socket")?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_linux_group(
    metadata: &std::fs::Metadata,
    expected_group: &str,
    kind: &str,
) -> Result<(), std::io::Error> {
    let group_file = std::fs::read_to_string("/etc/group")?;
    let expected_gid = group_file
        .lines()
        .find_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let gid = fields.next()?;
            (name == expected_group)
                .then_some(gid)
                .and_then(|value| value.parse::<u32>().ok())
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("configured gym MCP group {expected_group:?} does not exist"),
            )
        })?;
    if metadata.st_gid() != expected_gid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "gym MCP {kind} group id {} does not match {expected_group} ({expected_gid})",
                metadata.st_gid()
            ),
        ));
    }
    Ok(())
}

/// Opens the Unix byte stream used by an actual rmcp client.
///
/// # Errors
///
/// Returns [`std::io::Error`] when the socket cannot be reached.
pub async fn connect_mcp_socket(path: impl AsRef<Path>) -> std::io::Result<McpSocketStream> {
    UnixStream::connect(path).await
}

/// Exercises rmcp's production JSON-RPC request decoder for untrusted frames.
///
/// # Errors
///
/// Returns [`serde_json::Error`] when the frame is not a valid MCP client
/// message.
pub fn decode_mcp_frame(frame: &[u8]) -> Result<(), serde_json::Error> {
    serde_json::from_slice::<rmcp::model::ClientJsonRpcMessage>(frame).map(|_| ())
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        // Best effort: the socket may already be gone and `Drop` cannot report errors.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Bounded MCP query failures.
#[derive(Debug, Error)]
pub enum McpQueryError {
    /// Metric names must use the schema's bounded identifier grammar.
    #[error(
        "metric must be 1..={MAX_METRIC_LENGTH} lowercase ASCII letters, digits, or underscores"
    )]
    InvalidMetric,
    /// The requested day window is outside the published tool contract.
    #[error("days must be between 1 and {MAX_DAYS}, got {0}")]
    DaysOutOfRange(u16),
    /// The requested row cap is outside the published tool contract.
    #[error("limit must be between 1 and {MAX_LIMIT}, got {0}")]
    LimitOutOfRange(u16),
    /// Injected clock did not return RFC-3339.
    #[error("body_metrics clock did not return RFC-3339: {0}")]
    ClockTimestamp(#[from] chrono::ParseError),
    /// Subtracting the bounded lookback exceeded the timestamp range.
    #[error("body_metrics clock cannot represent the requested lookback")]
    ClockOutOfRange,
    /// A row in the frozen v0 database does not contain an RFC-3339 timestamp.
    #[error("stored body metric timestamp is not RFC-3339: {0}")]
    StoredTimestamp(String),
    /// The gym database failed validation or access.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// The fixed read query failed.
    #[error("body_metrics query failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Unix-socket server failures.
#[derive(Debug, Error)]
pub enum McpSocketError {
    /// Unix socket bind or accept failed.
    #[error("gym MCP socket error: {0}")]
    Io(#[from] std::io::Error),
    /// Existing gym storage failed startup validation.
    #[error(transparent)]
    Database(#[from] DatabaseError),
}

/// Returns the exact MCP capability map used by contract tests.
#[must_use]
pub fn capability_summary() -> HashMap<&'static str, bool> {
    HashMap::from([
        ("tools", true),
        ("resources", false),
        ("prompts", false),
        ("sampling", false),
    ])
}
