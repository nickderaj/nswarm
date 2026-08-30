//! Bounded read-only gym MCP surface over a Unix domain socket.

use std::{
    collections::HashMap,
    future::Future,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

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
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::net::{UnixListener, UnixStream};

use crate::{
    clock::Clock,
    database::{DatabaseError, open_existing_read_only, validate_existing},
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
    /// read query fails.
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
            .with_instructions("One bounded read-only body_metrics tool; no other surfaces")
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(vec![
            body_metrics_tool(),
        ])))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != "body_metrics" {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "unknown gym tool",
                None,
            ));
        }
        let arguments = request.arguments.unwrap_or_default();
        let args: BodyMetricsArgs = serde_json::from_value(Value::Object(arguments))
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
        Ok(CallToolResult::structured(structured).into())
    }
}

fn body_metrics_tool() -> Tool {
    let schema: JsonObject = serde_json::from_value(json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "metric": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_METRIC_LENGTH,
                "pattern": "^[a-z0-9_]+$"
            },
            "days": {"type": "integer", "minimum": 1, "maximum": MAX_DAYS},
            "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT}
        }
    }))
    .expect("static tool schema is an object");
    Tool::new(
        "body_metrics",
        "Read recent body metrics with bounded filters",
        schema,
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
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
    database_path: impl Into<PathBuf>,
    clock: Arc<dyn Clock>,
) -> Result<(), McpSocketError> {
    McpServer::bind(socket_path, database_path, clock)?
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
        let socket_path = socket_path.as_ref().to_path_buf();
        let database_path = database_path.into();
        validate_existing(&database_path)?;
        ensure_private_socket_parent(&socket_path)?;
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            _guard: SocketGuard(socket_path),
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

fn ensure_private_socket_parent(socket_path: &Path) -> Result<(), std::io::Error> {
    let parent = socket_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "gym MCP socket needs a parent directory",
        )
    })?;
    let mode = std::fs::metadata(parent)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("gym MCP socket directory mode {mode:o} permits group or other access"),
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
