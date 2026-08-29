//! Bounded read-only gym MCP surface over a Unix domain socket.

use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

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
    database::{DatabaseError, open_existing_read_only},
};

/// Default v0-compatible lookback in days.
pub const DEFAULT_DAYS: u16 = 56;
/// Maximum accepted lookback in days.
pub const MAX_DAYS: u16 = 365;
/// Default and maximum number of returned body-metric rows.
pub const DEFAULT_LIMIT: u16 = 200;
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
            .map(|metric| validate_metric(metric.trim()))
            .transpose()?;
        let days = self.days.unwrap_or(DEFAULT_DAYS);
        if !(1..=MAX_DAYS).contains(&days) {
            return Err(McpQueryError::DaysOutOfRange(days));
        }
        let limit = self.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=DEFAULT_LIMIT).contains(&limit) {
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
        let modifier = format!("-{} days", args.days);
        let now = self.clock.now_iso8601();
        let mut rows = Vec::new();
        if let Some(metric) = args.metric {
            let mut statement = connection.prepare(
                "SELECT date, metric, value, unit, source FROM body_metrics \
                 WHERE datetime(date) >= datetime(?1, ?2) AND metric = ?3 \
                 ORDER BY datetime(date) DESC, id DESC LIMIT ?4",
            )?;
            let mapped = statement.query_map(
                params![now, modifier, metric, i64::from(args.limit)],
                metric_from_row,
            )?;
            for row in mapped {
                rows.push(row?);
            }
        } else {
            let mut statement = connection.prepare(
                "SELECT date, metric, value, unit, source FROM body_metrics \
                 WHERE datetime(date) >= datetime(?1, ?2) \
                 ORDER BY datetime(date) DESC, id DESC LIMIT ?3",
            )?;
            let mapped = statement.query_map(
                params![now, modifier, i64::from(args.limit)],
                metric_from_row,
            )?;
            for row in mapped {
                rows.push(row?);
            }
        }
        Ok(rows)
    }
}

fn metric_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BodyMetric> {
    Ok(BodyMetric {
        date: row.get(0)?,
        metric: row.get(1)?,
        value: row.get(2)?,
        unit: row.get(3)?,
        source: row.get(4)?,
    })
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
        let rows = self
            .body_metrics(args)
            .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
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
            "limit": {"type": "integer", "minimum": 1, "maximum": DEFAULT_LIMIT}
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
        let listener = UnixListener::bind(&socket_path)?;
        Ok(Self {
            listener,
            _guard: SocketGuard(socket_path),
            database_path: database_path.into(),
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
        if let Err(error) = std::fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("could not remove MCP socket: {error}");
        }
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
    #[error("limit must be between 1 and {DEFAULT_LIMIT}, got {0}")]
    LimitOutOfRange(u16),
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
