//! Real rmcp client/server Unix-socket integration and query property tests.

mod common;

use std::{
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    sync::Arc,
};

use gym_bot::{
    clock::FixedClock,
    database::open_existing,
    mcp::{
        BodyMetricsArgs, GymMcp, MAX_DAYS, MAX_LIMIT, MAX_METRIC_LENGTH, McpServer,
        capability_summary, connect_mcp_socket, decode_mcp_frame,
    },
};
use proptest::prelude::*;
use rmcp::{
    ServiceError, ServiceExt,
    model::{CallToolRequestParams, ErrorCode},
};

const FIXED_TIME: &str = "2026-08-29T08:15:30+00:00";

#[test]
fn metric_length_boundary_is_exact() {
    let maximum = "a".repeat(MAX_METRIC_LENGTH);
    assert!(
        BodyMetricsArgs {
            metric: Some(maximum),
            days: None,
            limit: None,
        }
        .validate()
        .is_ok()
    );
    let oversized = "a".repeat(MAX_METRIC_LENGTH + 1);
    assert!(
        BodyMetricsArgs {
            metric: Some(oversized),
            days: None,
            limit: None,
        }
        .validate()
        .is_err()
    );
}

#[test]
fn production_query_filters_and_caps_rows() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = open_existing(&database).expect("open fixture");
    connection
        .execute_batch(
            "INSERT INTO body_metrics (date, metric, value, unit, source) VALUES \
             ('2026-08-29T08:15:30+00:00', 'weight_kg', 82.5, 'kg', 'manual'), \
             ('2026-08-28T08:15:30+00:00', 'weight_kg', 83.0, 'kg', 'manual'), \
             ('2026-08-27T08:15:30+00:00', 'resting_hr', 55.0, 'bpm', 'health');",
        )
        .expect("seed sanitized metrics");
    drop(connection);
    let service = GymMcp::new(&database, Arc::new(FixedClock::new(FIXED_TIME)));

    let rows = service
        .body_metrics(BodyMetricsArgs {
            metric: Some("weight_kg".to_owned()),
            days: Some(7),
            limit: Some(1),
        })
        .expect("bounded query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].date, FIXED_TIME);
    assert_eq!(rows[0].metric, "weight_kg");

    let all_rows = service
        .body_metrics(BodyMetricsArgs::default())
        .expect("default unfiltered query");
    assert_eq!(all_rows.len(), 3);
    assert_eq!(all_rows[2].metric, "resting_hr");

    let query_plan = open_existing(&database)
        .expect("open fixture for query plan")
        .query_row(
            "EXPLAIN QUERY PLAN \
             SELECT date, metric, value, unit, source FROM body_metrics \
             WHERE metric = ?1 AND date >= ?2 \
             ORDER BY date DESC, id DESC LIMIT ?3",
            ("weight_kg", "2026-08-22T08:15:30.000000+00:00", 200),
            |row| row.get::<_, String>(3),
        )
        .expect("metric query plan");
    assert!(
        query_plan.contains("body_metrics_metric_date"),
        "bounded metric query must use the existing composite index: {query_plan}"
    );
}

#[test]
fn actual_rmcp_client_and_server_exchange_protocol_over_unix_socket() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    open_existing(&database)
        .expect("open fixture")
        .execute(
            "INSERT INTO body_metrics (date, metric, value, unit, source) \
             VALUES (?1, 'weight_kg', 82.5, 'kg', 'manual')",
            [FIXED_TIME],
        )
        .expect("seed metric");
    let socket = private_socket_path(&directory);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let bound = McpServer::bind(&socket, &database, Arc::new(FixedClock::new(FIXED_TIME)))
            .expect("bind production socket");
        assert!(socket.exists());
        assert_private_socket_mode(&socket);
        let server = tokio::spawn(bound.run());
        let stream = connect_mcp_socket(&socket)
            .await
            .expect("connect Unix socket");
        let client = ().serve(stream).await.expect("real MCP handshake");

        let peer = client.peer_info().expect("server initialization info");
        assert!(peer.capabilities.tools.is_some());
        assert!(peer.capabilities.resources.is_none());
        assert!(peer.capabilities.prompts.is_none());
        assert!(peer.capabilities.logging.is_none());
        assert_eq!(
            capability_summary(),
            std::collections::HashMap::from([
                ("tools", true),
                ("resources", false),
                ("prompts", false),
                ("sampling", false),
            ])
        );

        let listed = client
            .list_tools(None)
            .await
            .expect("tools/list over socket");
        assert_eq!(listed.tools.len(), 1);
        let tool = &listed.tools[0];
        assert_eq!(tool.name, "body_metrics");
        assert_eq!(
            tool.annotations
                .as_ref()
                .and_then(|value| value.read_only_hint),
            Some(true)
        );
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert_eq!(tool.input_schema["properties"]["days"]["maximum"], MAX_DAYS);
        assert_eq!(
            tool.input_schema["properties"]["limit"]["maximum"],
            MAX_LIMIT
        );

        let result = client
            .call_tool(
                CallToolRequestParams::new("body_metrics").with_arguments(arguments(
                    &serde_json::json!({"metric":"weight_kg","days":1,"limit":1}),
                )),
            )
            .await
            .expect("tools/call over socket");
        let structured = result.structured_content.expect("structured body metrics");
        assert_eq!(structured.as_array().expect("row array").len(), 1);
        assert_eq!(structured[0]["value"], 82.5);

        assert_eq!(
            protocol_error_code(
                client
                    .call_tool(
                        CallToolRequestParams::new("body_metrics")
                            .with_arguments(arguments(&serde_json::json!({"days":0}))),
                    )
                    .await
                    .expect_err("zero days is a protocol error"),
            ),
            ErrorCode::INVALID_PARAMS
        );
        assert_eq!(
            protocol_error_code(
                client
                    .call_tool(CallToolRequestParams::new("filesystem"))
                    .await
                    .expect_err("unknown tool is a protocol error"),
            ),
            ErrorCode::METHOD_NOT_FOUND
        );

        client.cancel().await.expect("close MCP client");
        server.abort();
        assert!(server.await.expect_err("cancel server").is_cancelled());
        tokio::task::yield_now().await;
        assert!(
            !socket.exists(),
            "socket guard removes cancelled server path"
        );
    });
}

#[test]
fn socket_bind_rejects_a_directory_accessible_to_other_identities() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
        .expect("make test directory intentionally public");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .expect("runtime");

    let error = runtime
        .block_on(async {
            McpServer::bind(
                directory.path().join("mcp.sock"),
                database,
                Arc::new(FixedClock::new(FIXED_TIME)),
            )
        })
        .err()
        .expect("public parent directory must be rejected");
    assert_eq!(
        error.to_string(),
        "gym MCP socket error: gym MCP socket directory mode 755 permits group or other access"
    );
}

#[test]
fn storage_failure_is_an_internal_protocol_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let socket = private_socket_path(&directory);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let bound = McpServer::bind(&socket, &database, Arc::new(FixedClock::new(FIXED_TIME)))
            .expect("bind production socket");
        let server = tokio::spawn(bound.run());
        let client = ()
            .serve(
                connect_mcp_socket(&socket)
                    .await
                    .expect("connect Unix socket"),
            )
            .await
            .expect("real MCP handshake");
        std::fs::rename(&database, directory.path().join("gym-unavailable.db"))
            .expect("make database unavailable");

        assert_eq!(
            protocol_error_code(
                client
                    .call_tool(CallToolRequestParams::new("body_metrics"))
                    .await
                    .expect_err("storage failure is a protocol error"),
            ),
            ErrorCode::INTERNAL_ERROR
        );

        client.cancel().await.expect("close MCP client");
        server.abort();
        assert!(server.await.expect_err("cancel server").is_cancelled());
    });
}

#[test]
fn rmcp_request_decoder_rejects_malformed_frames() {
    assert!(decode_mcp_frame(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).is_ok());
    for malformed in [
        b"not-json".as_slice(),
        br#"{"jsonrpc":"2.0","id":1}"#,
        br#"{"jsonrpc":"2.0","id":1,"method":7}"#,
    ] {
        assert!(decode_mcp_frame(malformed).is_err());
    }
}

fn arguments(value: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().expect("test arguments object").clone()
}

fn private_socket_path(directory: &tempfile::TempDir) -> std::path::PathBuf {
    let socket_directory = directory.path().join("socket");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&socket_directory)
        .expect("private socket directory");
    socket_directory.join("mcp.sock")
}

#[cfg(target_os = "linux")]
fn assert_private_socket_mode(socket: &std::path::Path) {
    assert_eq!(
        std::fs::metadata(socket)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the Step 2 socket is private to its service identity"
    );
}

#[cfg(not(target_os = "linux"))]
const fn assert_private_socket_mode(_socket: &std::path::Path) {}

fn protocol_error_code(error: ServiceError) -> ErrorCode {
    match error {
        ServiceError::McpError(error) => error.code,
        other => panic!("expected MCP protocol error, got {other:?}"),
    }
}

proptest! {
    #[test]
    fn numeric_query_bounds_are_total(days in any::<u16>(), limit in any::<u16>()) {
        let valid = BodyMetricsArgs {
            metric: None,
            days: Some(days),
            limit: Some(limit),
        }.validate().is_ok();
        prop_assert_eq!(valid, (1..=MAX_DAYS).contains(&days) && (1..=MAX_LIMIT).contains(&limit));
    }

    #[test]
    fn accepted_metric_names_follow_the_published_grammar(metric in ".{0,100}") {
        let valid = BodyMetricsArgs {
            metric: Some(metric.clone()),
            days: None,
            limit: None,
        }.validate().is_ok();
        let expected = !metric.is_empty()
            && metric.len() <= 64
            && metric.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
            });
        prop_assert_eq!(valid, expected);
    }
}
