//! Real rmcp client/server Unix-socket integration and query property tests.

mod common;

use std::sync::Arc;

use gym_bot::{
    clock::FixedClock,
    database::open_existing,
    mcp::{
        BodyMetricsArgs, DEFAULT_LIMIT, GymMcp, MAX_DAYS, McpServer, capability_summary,
        connect_mcp_socket, decode_mcp_frame,
    },
};
use proptest::prelude::*;
use rmcp::{
    ServiceError, ServiceExt,
    model::{CallToolRequestParams, ErrorCode},
};

const FIXED_TIME: &str = "2026-08-29T08:15:30+00:00";

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
    let socket = directory.path().join("mcp.sock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let bound = McpServer::bind(&socket, &database, Arc::new(FixedClock::new(FIXED_TIME)))
            .expect("bind production socket");
        assert!(socket.exists());
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
            DEFAULT_LIMIT
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
        prop_assert_eq!(valid, (1..=MAX_DAYS).contains(&days) && (1..=DEFAULT_LIMIT).contains(&limit));
    }

    #[test]
    fn accepted_metric_names_follow_the_published_grammar(metric in ".{0,100}") {
        let valid = BodyMetricsArgs {
            metric: Some(metric.clone()),
            days: None,
            limit: None,
        }.validate().is_ok();
        let expected = !metric.trim().is_empty()
            && metric.trim().len() <= 64
            && metric.trim().bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
            });
        prop_assert_eq!(valid, expected);
    }
}
