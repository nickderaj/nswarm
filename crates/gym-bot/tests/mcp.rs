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
        BodyMetricsArgs, GYM_TOOL_NAMES, GymMcp, MAX_DAYS, MAX_LIMIT, MAX_METRIC_LENGTH,
        McpQueryError, McpServer, capability_summary, connect_mcp_socket, decode_mcp_frame,
        run_mcp_server, run_mcp_server_for_group,
    },
};
use proptest::prelude::*;
use rmcp::{
    ServiceError, ServiceExt,
    model::{CallToolRequestParams, ErrorCode},
};

const FIXED_TIME: &str = "2026-08-29T08:15:30+00:00";

#[test]
fn public_server_wrappers_accept_real_mcp_connections() {
    for group_aware in [false, true] {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = common::copy_fixture(&directory, "gym.db");
        let socket = private_socket_path(&directory);
        std::fs::set_permissions(
            socket.parent().expect("socket parent"),
            std::fs::Permissions::from_mode(0o2750),
        )
        .expect("setgid socket parent");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let clock = Arc::new(FixedClock::new(FIXED_TIME));
            let server = if group_aware {
                tokio::spawn(run_mcp_server_for_group(
                    socket.clone(),
                    current_group(),
                    database,
                    clock,
                ))
            } else {
                tokio::spawn(run_mcp_server(socket.clone(), database, clock))
            };
            for _ in 0..100 {
                if socket.exists() {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let client =
                ().serve(connect_mcp_socket(&socket).await.expect("connect"))
                    .await
                    .expect("handshake");
            client.cancel().await.expect("close");
            server.abort();
        });
    }
}

#[cfg(target_os = "linux")]
fn current_group() -> &'static str {
    use std::sync::OnceLock;
    static GROUP: OnceLock<String> = OnceLock::new();
    GROUP.get_or_init(|| {
        String::from_utf8(
            std::process::Command::new("id")
                .arg("-gn")
                .output()
                .expect("query group")
                .stdout,
        )
        .expect("UTF-8 group")
        .trim()
        .to_owned()
    })
}

#[cfg(not(target_os = "linux"))]
const fn current_group() -> &'static str {
    "fixture-group"
}

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
             SELECT id, date, metric, value, unit, source FROM body_metrics \
             WHERE metric = ?1 AND date >= ?2 \
             ORDER BY date DESC, id DESC",
            ("weight_kg", "2026-08-20"),
            |row| row.get::<_, String>(3),
        )
        .expect("metric query plan");
    assert!(
        query_plan.contains("body_metrics_metric_date"),
        "bounded metric query must use the existing composite index: {query_plan}"
    );
}

#[test]
fn query_filters_and_orders_by_instant_across_offsets() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "dst.db");
    open_existing(&database)
        .expect("open fixture")
        .execute_batch(
            "INSERT INTO body_metrics (date, metric, value, unit, source) VALUES \
             ('2026-10-25T01:29:59+01:00', 'weight_kg', 70, 'kg', 'manual'), \
             ('2026-10-25T01:30:00+01:00', 'weight_kg', 71, 'kg', 'manual'), \
             ('2026-10-25T01:30:00+00:00', 'weight_kg', 72, 'kg', 'manual');",
        )
        .expect("seed transition rows");
    let service = GymMcp::new(
        &database,
        Arc::new(FixedClock::new("2026-10-26T00:30:00+00:00")),
    );

    let rows = service
        .body_metrics(BodyMetricsArgs {
            metric: Some("weight_kg".to_owned()),
            days: Some(1),
            limit: Some(10),
        })
        .expect("instant-correct query");
    assert_eq!(
        rows.iter().map(|row| row.value).collect::<Vec<_>>(),
        vec![72.0, 71.0]
    );
}

#[test]
fn query_includes_a_whole_second_exactly_at_the_cutoff() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "cutoff.db");
    open_existing(&database)
        .expect("open fixture")
        .execute(
            "INSERT INTO body_metrics (date, metric, value, unit, source) \
             VALUES ('2026-07-04T09:15:30+01:00', 'weight_kg', 73, 'kg', 'manual')",
            [],
        )
        .expect("seed exact-cutoff row");
    let service = GymMcp::new(
        &database,
        Arc::new(FixedClock::new("2026-07-05T08:15:30+00:00")),
    );

    let rows = service
        .body_metrics(BodyMetricsArgs {
            metric: Some("weight_kg".to_owned()),
            days: Some(1),
            limit: Some(10),
        })
        .expect("exact-cutoff query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.to_bits(), 73.0_f64.to_bits());
}

#[test]
fn selected_non_rfc3339_timestamp_fails_the_whole_query() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "offset-free.db");
    open_existing(&database)
        .expect("open fixture")
        .execute_batch(
            "INSERT INTO body_metrics (date, metric, value, unit, source) VALUES \
             ('2026-08-29T08:15:30+00:00', 'weight_kg', 82, 'kg', 'manual'), \
             ('2026-08-29T08:15:30', 'weight_kg', 83, 'kg', 'legacy');",
        )
        .expect("seed valid and offset-free rows");
    let service = GymMcp::new(&database, Arc::new(FixedClock::new(FIXED_TIME)));

    let error = service
        .body_metrics(BodyMetricsArgs::default())
        .expect_err("an offset-free candidate must fail instead of being omitted");
    assert!(matches!(
        error,
        McpQueryError::StoredTimestamp(value) if value == "2026-08-29T08:15:30"
    ));
}

#[test]
fn malformed_timestamp_outside_the_candidate_window_is_not_a_table_audit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "out-of-window.db");
    open_existing(&database)
        .expect("open fixture")
        .execute_batch(
            "INSERT INTO body_metrics (date, metric, value, unit, source) VALUES \
             ('0000-not-a-time', 'weight_kg', 81, 'kg', 'legacy'), \
             ('2026-08-29T08:15:30+00:00', 'weight_kg', 82, 'kg', 'manual');",
        )
        .expect("seed out-of-window malformed row");
    let service = GymMcp::new(&database, Arc::new(FixedClock::new(FIXED_TIME)));

    let rows = service
        .body_metrics(BodyMetricsArgs::default())
        .expect("only selected candidates are validated");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].date, FIXED_TIME);
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
        assert_private_socket_boundary(&socket);
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

        assert_reviewed_tool_list(&client).await;

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

        assert_reviewed_write(&client).await;

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
        assert_body_metric_boundaries(&client).await;
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
fn volume_summary_uses_exact_per_week_group_limit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let mut connection = open_existing(&database).expect("open fixture");
    let transaction = connection.transaction().expect("seed transaction");
    for index in 0..54 {
        transaction
            .execute(
                "INSERT INTO sessions (started_at,kind,source) \
                 VALUES ('2026-08-29T08:15:30+00:00','strength','manual')",
                [],
            )
            .expect("session");
        let session_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO movements (name,display_name,modality,muscle_groups) \
                 VALUES (?1,?1,'strength',?2)",
                rusqlite::params![format!("movement-{index}"), format!("group-{index}")],
            )
            .expect("movement");
        let movement_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO session_items (session_id,position,movement_id) VALUES (?1,1,?2)",
                rusqlite::params![session_id, movement_id],
            )
            .expect("session item");
        let item_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO efforts (session_item_id,position,reps,weight_kg) \
                 VALUES (?1,1,1,1)",
                [item_id],
            )
            .expect("effort");
    }
    transaction.commit().expect("seed commit");
    drop(connection);

    let socket = private_socket_path(&directory);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = tokio::spawn(
            McpServer::bind(&socket, database, Arc::new(FixedClock::new(FIXED_TIME)))
                .expect("bind")
                .run(),
        );
        let client =
            ().serve(connect_mcp_socket(&socket).await.expect("connect"))
                .await
                .expect("handshake");
        let result = client
            .call_tool(
                CallToolRequestParams::new("volume_summary")
                    .with_arguments(arguments(&serde_json::json!({"weeks":1}))),
            )
            .await
            .expect("volume summary");
        assert_eq!(
            result
                .structured_content
                .expect("structured result")
                .as_array()
                .expect("rows")
                .len(),
            53
        );
        client.cancel().await.expect("close");
        server.abort();
    });
}

async fn assert_body_metric_boundaries(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) {
    for arguments_value in [
        serde_json::json!({"metric":"BAD"}),
        serde_json::json!({"days":366}),
        serde_json::json!({"limit":201}),
    ] {
        assert_eq!(
            protocol_error_code(
                client
                    .call_tool(
                        CallToolRequestParams::new("body_metrics")
                            .with_arguments(arguments(&arguments_value)),
                    )
                    .await
                    .expect_err("bounded body metrics")
            ),
            ErrorCode::INVALID_PARAMS
        );
    }
}

async fn assert_reviewed_tool_list(client: &rmcp::service::RunningService<rmcp::RoleClient, ()>) {
    let listed = client
        .list_tools(None)
        .await
        .expect("tools/list over socket");
    assert_eq!(
        listed
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<std::collections::BTreeSet<_>>(),
        GYM_TOOL_NAMES.into_iter().collect()
    );
    let tool = listed
        .tools
        .iter()
        .find(|tool| tool.name == "body_metrics")
        .expect("body_metrics tool");
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
    for listed_tool in &listed.tools {
        let write = matches!(
            listed_tool.name.as_ref(),
            "record_preference" | "propose_plan"
        );
        let annotations = listed_tool.annotations.as_ref().expect("tool annotations");
        assert_eq!(
            annotations.read_only_hint,
            Some(!write),
            "{}",
            listed_tool.name
        );
        assert_eq!(
            annotations.idempotent_hint,
            Some(!write),
            "{}",
            listed_tool.name
        );
        assert_eq!(
            annotations.destructive_hint,
            Some(false),
            "{}",
            listed_tool.name
        );
        assert_eq!(
            annotations.open_world_hint,
            Some(false),
            "{}",
            listed_tool.name
        );
    }
    for (name, property) in [
        ("record_preference", "evidence"),
        ("propose_plan", "items"),
        ("plan_feedback", "plan_id"),
    ] {
        let reviewed_tool = listed
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("reviewed schema");
        assert_eq!(reviewed_tool.input_schema["additionalProperties"], false);
        assert!(
            reviewed_tool.input_schema["properties"]
                .as_object()
                .is_some_and(|properties| properties.contains_key(property)),
            "{name} must publish {property}"
        );
    }
}

async fn assert_reviewed_write(client: &rmcp::service::RunningService<rmcp::RoleClient, ()>) {
    let preference = client
        .call_tool(
            CallToolRequestParams::new("record_preference").with_arguments(arguments(
                &serde_json::json!({
                    "key":"warmup",
                    "value":"short",
                    "evidence":"owner stated this in the current turn"
                }),
            )),
        )
        .await
        .expect("reviewed write over socket");
    assert_eq!(
        preference.structured_content.expect("write result")["id"],
        1
    );
    let preferences = client
        .call_tool(CallToolRequestParams::new("preferences"))
        .await
        .expect("read preference");
    assert_eq!(
        preferences.structured_content.expect("preferences")[0]["key"],
        "warmup"
    );
    let plan = client
        .call_tool(
            CallToolRequestParams::new("propose_plan").with_arguments(arguments(
                &serde_json::json!({"focus":"legs","rationale":"fixture","for_date":"2026-08-31","items":[{"exercise":"squat","sets":[{"reps":5}]}]}),
            )),
        )
        .await
        .expect("propose plan");
    let id = plan.structured_content.expect("plan result")["id"]
        .as_i64()
        .expect("plan id");
    let feedback = client
        .call_tool(
            CallToolRequestParams::new("plan_feedback")
                .with_arguments(arguments(&serde_json::json!({"plan_id":id}))),
        )
        .await
        .expect("plan feedback");
    assert_eq!(
        feedback.structured_content.expect("feedback result")["status"],
        "proposed"
    );
}

#[test]
fn every_reviewed_read_tool_executes_fixed_sql() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let socket = private_socket_path(&directory);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = tokio::spawn(
            McpServer::bind(&socket, &database, Arc::new(FixedClock::new(FIXED_TIME)))
                .expect("bind")
                .run(),
        );
        let client =
            ().serve(connect_mcp_socket(&socket).await.expect("connect"))
                .await
                .expect("handshake");
        for tool in GYM_TOOL_NAMES.into_iter().filter(|name| {
            !matches!(
                *name,
                "body_metrics" | "record_preference" | "propose_plan" | "plan_feedback"
            )
        }) {
            let result = client
                .call_tool(CallToolRequestParams::new(tool))
                .await
                .expect("fixed read tool");
            assert!(result.structured_content.is_some(), "{tool}");
        }
        for (tool, arguments_value) in [
            ("recent_sets", serde_json::json!({"limit":0})),
            (
                "record_preference",
                serde_json::json!({"key":"","value":"x","evidence":"x"}),
            ),
            (
                "propose_plan",
                serde_json::json!({"focus":"x","rationale":"x","items":[]}),
            ),
            ("plan_feedback", serde_json::json!({"plan_id":999})),
            ("exercise_catalogue", serde_json::json!({"unexpected":true})),
            (
                "record_preference",
                serde_json::json!({"key":"x","value":"x","evidence":"x","unexpected":true}),
            ),
            (
                "propose_plan",
                serde_json::json!({"focus":"x","rationale":"x","items":[{}],"unexpected":true}),
            ),
            (
                "plan_feedback",
                serde_json::json!({"plan_id":1,"unexpected":true}),
            ),
            ("recent_sets", serde_json::json!({"limit":"bad"})),
            ("body_metrics", serde_json::json!({"unknown":true})),
            (
                "record_preference",
                serde_json::json!({"key":"x","value":"x"}),
            ),
            (
                "propose_plan",
                serde_json::json!({"focus":"","rationale":"x","items":[{}]}),
            ),
            ("interval_history", serde_json::json!({"limit":201})),
            ("heart_rate_series", serde_json::json!({"samples":0})),
        ] {
            assert_eq!(
                protocol_error_code(
                    client
                        .call_tool(
                            CallToolRequestParams::new(tool)
                                .with_arguments(arguments(&arguments_value))
                        )
                        .await
                        .expect_err("invalid args")
                ),
                ErrorCode::INVALID_PARAMS
            );
        }
        for (tool, arguments_value) in [
            ("volume_summary", serde_json::json!({"weeks":52})),
            ("interval_history", serde_json::json!({"limit":200})),
            ("heart_rate_series", serde_json::json!({"samples":5000})),
        ] {
            assert!(
                client
                    .call_tool(
                        CallToolRequestParams::new(tool)
                            .with_arguments(arguments(&arguments_value))
                    )
                    .await
                    .expect("bounded read")
                    .structured_content
                    .is_some()
            );
        }
        client.cancel().await.expect("close");
        server.abort();
    });
}

#[test]
fn socket_bind_rejects_a_world_accessible_directory() {
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
        "gym MCP socket error: gym MCP socket directory mode 755 must be 750"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn production_bind_fails_closed_for_an_unknown_group() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o2750))
        .expect("setgid test directory");
    let error = McpServer::bind_for_group(
        directory.path().join("mcp.sock"),
        "nswarm-group-that-must-not-exist",
        database,
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .err()
    .expect("unknown production group must fail");
    assert!(error.to_string().contains("does not exist"));
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
        assert_eq!(
            protocol_error_code(
                client
                    .call_tool(
                        CallToolRequestParams::new("record_preference").with_arguments(arguments(
                            &serde_json::json!({"key":"x","value":"x","evidence":"x"}),
                        )),
                    )
                    .await
                    .expect_err("write storage failure")
            ),
            ErrorCode::INTERNAL_ERROR
        );

        client.cancel().await.expect("close MCP client");
        server.abort();
        assert!(server.await.expect_err("cancel server").is_cancelled());
    });
}

#[test]
fn invalid_clock_is_an_internal_protocol_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let socket = private_socket_path(&directory);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = tokio::spawn(
            McpServer::bind(&socket, database, Arc::new(FixedClock::new("invalid")))
                .expect("bind")
                .run(),
        );
        let client =
            ().serve(connect_mcp_socket(&socket).await.expect("connect"))
                .await
                .expect("handshake");
        assert_eq!(
            protocol_error_code(
                client
                    .call_tool(CallToolRequestParams::new("body_metrics"))
                    .await
                    .expect_err("invalid clock")
            ),
            ErrorCode::INTERNAL_ERROR
        );
        client.cancel().await.expect("close");
        server.abort();
    });
}

#[test]
fn clock_range_overflow_fails_closed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymMcp::new(
        database,
        Arc::new(FixedClock::new("-262143-01-01T00:00:00+00:00")),
    );
    assert!(service.body_metrics(BodyMetricsArgs::default()).is_err());
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
        .mode(0o750)
        .create(&socket_directory)
        .expect("private socket directory");
    std::fs::set_permissions(&socket_directory, std::fs::Permissions::from_mode(0o750))
        .expect("exact group socket directory permissions");
    socket_directory.join("mcp.sock")
}

fn assert_private_socket_boundary(socket: &std::path::Path) {
    assert_eq!(
        std::fs::metadata(socket.parent().expect("socket parent"))
            .expect("socket parent metadata")
            .permissions()
            .mode()
            & 0o777,
        0o750,
        "the runtime directory admits only the service and socket group"
    );
    assert_eq!(
        std::fs::metadata(socket)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777,
        0o660,
        "the Step 4 socket is shared only with its authorized group"
    );
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
