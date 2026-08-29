//! Intent-level deterministic `SQLite` parity contract tests.

mod common;

use std::{path::Path, sync::Arc};

use botkit::{SurfaceId, UpdateKey};
use gym_bot::{
    clock::FixedClock,
    command::{CommandInput, CommandResult, CommandService},
    parity::{
        DifferenceAllowList, ParityError, ParityIntent, apply_v0_intent, compare_snapshots,
        normalize_database,
    },
};
use rusqlite::Connection;

fn intent_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym/log-body-weight-v1.json")
}

fn schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym/intent-v1.schema.json")
}

fn load_intent() -> ParityIntent {
    ParityIntent::from_json(&std::fs::read(intent_path()).expect("read committed intent"))
        .expect("machine-validated committed intent")
}

#[test]
fn fixed_time_weight_intent_has_exact_v0_v1_database_parity() {
    let directory = tempfile::tempdir().expect("tempdir");
    let baseline = common::copy_fixture(&directory, "v0.db");
    let candidate = common::copy_fixture(&directory, "v1.db");
    let intent = load_intent();

    apply_v0_intent(&baseline, &intent).expect("apply independent v0 baseline");
    let service = CommandService::new(
        "fixture-owner",
        &candidate,
        Arc::new(FixedClock::new(&intent.at)),
    );
    let result = service
        .handle(&CommandInput {
            actor_id: "fixture-owner".to_owned(),
            update: UpdateKey::new(
                SurfaceId::new("parity").expect("surface"),
                "log-body-weight-v1",
            )
            .expect("update key"),
            text: format!("/weight {}", intent.kilograms),
        })
        .expect("apply v1 intent");
    assert_eq!(
        result,
        CommandResult::Reply("✅ Logged weight: 82.5 kg".to_owned())
    );

    let baseline = normalize_database(&baseline).expect("normalize v0");
    let candidate = normalize_database(&candidate).expect("normalize v1");
    assert_eq!(baseline.schema_version, 5);
    assert_eq!(baseline.tables.len(), 15);
    assert_eq!(baseline.schema_objects.len(), 8);
    assert!(baseline.foreign_key_violations.is_empty());
    assert!(
        compare_snapshots(&baseline, &candidate, &DifferenceAllowList::empty())
            .expect("compare states")
            .is_empty()
    );
}

#[test]
fn committed_intent_schema_and_typed_validator_fail_closed() {
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(schema_path()).expect("read schema"))
            .expect("schema is machine-readable JSON");
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    assert_eq!(schema["properties"]["kind"]["const"], "log_body_weight");
    assert_eq!(load_intent().schema_version, 1);

    for invalid in [
        br#"{"schema_version":2,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","kilograms":80}"#.as_slice(),
        br#"{"schema_version":1,"kind":"log_body_weight","at":"not-a-time","kilograms":80}"#,
        br#"{"schema_version":1,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","kilograms":0}"#,
        br#"{"schema_version":1,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","kilograms":80,"unexpected":true}"#,
    ] {
        assert!(ParityIntent::from_json(invalid).is_err());
    }
    assert!(matches!(
        ParityIntent::from_json(br#"{"schema_version":2,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","kilograms":80}"#),
        Err(ParityError::IntentSchemaVersion { expected: 1, actual: 2 })
    ));
}

#[test]
fn missing_and_extra_rows_are_structural_differences() {
    let directory = tempfile::tempdir().expect("tempdir");
    let with_row = common::copy_fixture(&directory, "with-row.db");
    let empty = common::copy_fixture(&directory, "empty.db");
    apply_v0_intent(&with_row, &load_intent()).expect("apply baseline");

    let with_row = normalize_database(&with_row).expect("snapshot with row");
    let empty = normalize_database(&empty).expect("snapshot empty");
    let missing =
        compare_snapshots(&with_row, &empty, &DifferenceAllowList::empty()).expect("missing diff");
    let extra =
        compare_snapshots(&empty, &with_row, &DifferenceAllowList::empty()).expect("extra diff");
    assert!(
        missing
            .iter()
            .any(|diff| diff.path == "/tables/body_metrics/rows/0")
    );
    assert!(
        extra
            .iter()
            .any(|diff| diff.path == "/tables/body_metrics/rows/0")
    );
}

#[test]
fn unexpected_tables_schema_version_and_column_drift_are_detected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected_path = common::copy_fixture(&directory, "expected.db");
    let actual_path = common::copy_fixture(&directory, "actual.db");
    let connection = Connection::open(&actual_path).expect("open candidate");
    connection
        .execute_batch(
            "CREATE TABLE unexpected (id INTEGER PRIMARY KEY); \
             ALTER TABLE body_metrics ADD COLUMN surprise TEXT; \
             PRAGMA user_version=6;",
        )
        .expect("induce structural drift");
    drop(connection);

    let expected = normalize_database(&expected_path).expect("expected snapshot");
    let actual = normalize_database(&actual_path).expect("actual snapshot");
    let differences = compare_snapshots(&expected, &actual, &DifferenceAllowList::empty())
        .expect("structural diff");
    assert!(
        differences
            .iter()
            .any(|diff| diff.path == "/schema_version")
    );
    assert!(
        differences
            .iter()
            .any(|diff| diff.path == "/tables/unexpected")
    );
    assert!(differences.iter().any(|diff| {
        diff.path == "/tables/body_metrics/sql"
            || diff.path.starts_with("/tables/body_metrics/columns/")
    }));
}

#[test]
fn index_drift_is_detected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected_path = common::copy_fixture(&directory, "expected.db");
    let actual_path = common::copy_fixture(&directory, "actual.db");
    Connection::open(&actual_path)
        .expect("open candidate")
        .execute("DROP INDEX body_metrics_metric_date", [])
        .expect("induce index drift");

    let differences = compare_snapshots(
        &normalize_database(&expected_path).expect("expected snapshot"),
        &normalize_database(&actual_path).expect("actual snapshot"),
        &DifferenceAllowList::empty(),
    )
    .expect("index diff");
    assert!(
        differences
            .iter()
            .any(|diff| diff.path == "/schema_objects/index~1body_metrics_metric_date")
    );
}

#[test]
fn column_value_drift_is_detected_without_an_allow_list() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected_path = common::copy_fixture(&directory, "expected.db");
    let actual_path = common::copy_fixture(&directory, "actual.db");
    let intent = load_intent();
    apply_v0_intent(&expected_path, &intent).expect("expected row");
    apply_v0_intent(&actual_path, &intent).expect("actual row");
    Connection::open(&actual_path)
        .expect("open candidate")
        .execute("UPDATE body_metrics SET value=99 WHERE id=1", [])
        .expect("induce value drift");

    let differences = compare_snapshots(
        &normalize_database(&expected_path).expect("expected snapshot"),
        &normalize_database(&actual_path).expect("actual snapshot"),
        &DifferenceAllowList::empty(),
    )
    .expect("value diff");
    assert!(differences.iter().any(|diff| {
        diff.path.starts_with("/tables/body_metrics/rows/0/") && diff.expected != diff.actual
    }));
}

#[test]
fn foreign_key_failures_are_captured_and_diffed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected_path = common::copy_fixture(&directory, "expected.db");
    let actual_path = common::copy_fixture(&directory, "actual.db");
    let connection = Connection::open(&actual_path).expect("open candidate");
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF; \
             INSERT INTO session_items (session_id, position, movement_id) VALUES (99, 1, 88);",
        )
        .expect("induce foreign key violation");
    drop(connection);

    let expected = normalize_database(&expected_path).expect("expected snapshot");
    let actual = normalize_database(&actual_path).expect("actual snapshot");
    assert!(expected.foreign_key_violations.is_empty());
    assert_eq!(actual.foreign_key_violations.len(), 2);
    let differences = compare_snapshots(&expected, &actual, &DifferenceAllowList::empty())
        .expect("foreign key diff");
    assert!(
        differences
            .iter()
            .any(|diff| diff.path.starts_with("/foreign_key_violations/"))
    );
}

#[test]
fn only_explicit_exact_paths_can_be_ignored() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected_path = common::copy_fixture(&directory, "expected.db");
    let actual_path = common::copy_fixture(&directory, "actual.db");
    Connection::open(&actual_path)
        .expect("open candidate")
        .pragma_update(None, "user_version", 6)
        .expect("drift version");
    let expected = normalize_database(&expected_path).expect("expected snapshot");
    let actual = normalize_database(&actual_path).expect("actual snapshot");
    assert_eq!(
        compare_snapshots(
            &expected,
            &actual,
            &DifferenceAllowList::explicit(vec!["/schema_version".to_owned()]),
        )
        .expect("allow-listed diff"),
        Vec::new()
    );
}
