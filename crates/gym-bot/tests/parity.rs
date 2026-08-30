//! Intent-level deterministic `SQLite` parity contract tests.

mod common;

use std::{path::Path, sync::Arc};

use botkit::{SurfaceId, UpdateKey};
use gym_bot::{
    clock::{FixedClock, timestamp_in_timezone},
    command::{CommandInput, CommandResult, CommandService},
    parity::{
        CellValue, DifferenceAllowList, ParityError, ParityIntent, compare_snapshots,
        expected_v0_snapshot, normalize_database,
    },
};
use rusqlite::Connection;

fn intent_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym/log-body-weight-v1.json")
}

fn schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym/intent-v1.schema.json")
}

fn golden_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym/log-body-weight-v0-golden.json")
}

fn load_intent() -> ParityIntent {
    ParityIntent::from_json(&std::fs::read(intent_path()).expect("read committed intent"))
        .expect("machine-validated committed intent")
}

#[test]
fn golden_provenance_fields_are_validated_independently() {
    let directory = tempfile::tempdir().expect("tempdir");
    let fixture = common::copy_fixture(&directory, "empty.db");
    let bytes = std::fs::read(golden_path()).expect("read golden");
    let golden: serde_json::Value = serde_json::from_slice(&bytes).expect("parse golden");

    for (field, invalid) in [
        ("commit", ""),
        ("file", ""),
        ("sha256", "too-short"),
        ("generator", ""),
    ] {
        let mut candidate = golden.clone();
        candidate["source"][field] = serde_json::Value::String(invalid.to_owned());
        let candidate = serde_json::to_vec(&candidate).expect("serialize invalid golden");
        assert!(matches!(
            expected_v0_snapshot(&fixture, &load_intent(), &candidate),
            Err(ParityError::InvalidGoldenProvenance)
        ));
    }
}

#[test]
fn fixed_time_weight_intent_has_exact_v0_v1_database_parity() {
    let directory = tempfile::tempdir().expect("tempdir");
    let baseline_fixture = common::copy_fixture(&directory, "v0.db");
    let candidate = common::copy_fixture(&directory, "v1.db");
    let intent = load_intent();

    let baseline = expected_v0_snapshot(
        &baseline_fixture,
        &intent,
        &std::fs::read(golden_path()).expect("read committed v0 golden snapshot"),
    )
    .expect("build independent v0 baseline");
    let candidate_timestamp = timestamp_in_timezone(&intent.at, &intent.time_zone)
        .expect("convert candidate timestamp using production logic");
    let service = CommandService::new(
        "fixture-owner",
        &candidate,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(candidate_timestamp)),
    )
    .expect("command service");
    let result = service
        .handle(&CommandInput {
            actor_id: "fixture-owner".to_owned(),
            update: UpdateKey::new(
                SurfaceId::new("parity").expect("surface"),
                "log-body-weight-v1",
            )
            .expect("update key"),
            text: format!("/weight {}", intent.kilograms),
            conversation_id: "1001".to_owned(),
        })
        .expect("apply v1 intent");
    assert_eq!(
        result,
        CommandResult::Reply("✅ Logged weight: 82.5 kg".to_owned())
    );

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
fn utc_storage_regression_is_detected_against_the_v0_golden_snapshot() {
    let directory = tempfile::tempdir().expect("tempdir");
    let baseline_fixture = common::copy_fixture(&directory, "v0.db");
    let candidate = common::copy_fixture(&directory, "utc-v1.db");
    let intent = load_intent();
    let baseline = expected_v0_snapshot(
        &baseline_fixture,
        &intent,
        &std::fs::read(golden_path()).expect("read committed v0 golden snapshot"),
    )
    .expect("build independent v0 baseline");
    let service = CommandService::new(
        "fixture-owner",
        &candidate,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(&intent.at)),
    )
    .expect("command service with deliberately wrong UTC storage");
    service
        .handle(&CommandInput {
            actor_id: "fixture-owner".to_owned(),
            update: UpdateKey::new(SurfaceId::new("parity").expect("surface"), "utc-regression")
                .expect("update key"),
            text: format!("/weight {}", intent.kilograms),
            conversation_id: "1001".to_owned(),
        })
        .expect("apply deliberately divergent v1 intent");

    let differences = compare_snapshots(
        &baseline,
        &normalize_database(&candidate).expect("normalize UTC candidate"),
        &DifferenceAllowList::empty(),
    )
    .expect("compare states");
    assert!(differences.iter().any(|difference| {
        difference.path.starts_with("/tables/body_metrics/rows/0/1")
            && difference.expected != difference.actual
    }));
}

#[test]
fn committed_intent_schema_and_typed_validator_fail_closed() {
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(schema_path()).expect("read schema"))
            .expect("schema is machine-readable JSON");
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    assert_eq!(schema["properties"]["kind"]["const"], "log_body_weight");
    assert_eq!(schema["properties"]["time_zone"]["maxLength"], 128);
    assert_eq!(load_intent().schema_version, 1);

    for invalid in [
        br#"{"schema_version":2,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","time_zone":"Europe/London","kilograms":80}"#.as_slice(),
        br#"{"schema_version":1,"kind":"log_body_weight","at":"not-a-time","time_zone":"Europe/London","kilograms":80}"#,
        br#"{"schema_version":1,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","time_zone":"Not/AZone","kilograms":80}"#,
        br#"{"schema_version":1,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","time_zone":"Europe/London","kilograms":0}"#,
        br#"{"schema_version":1,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","time_zone":"Europe/London","kilograms":80,"unexpected":true}"#,
    ] {
        assert!(ParityIntent::from_json(invalid).is_err());
    }
    assert!(matches!(
        ParityIntent::from_json(br#"{"schema_version":2,"kind":"log_body_weight","at":"2026-08-29T08:15:30Z","time_zone":"Europe/London","kilograms":80}"#),
        Err(ParityError::IntentSchemaVersion { expected: 1, actual: 2 })
    ));
}

#[test]
fn missing_and_extra_rows_are_structural_differences() {
    let directory = tempfile::tempdir().expect("tempdir");
    let with_row = common::copy_fixture(&directory, "with-row.db");
    let empty = common::copy_fixture(&directory, "empty.db");

    let intent = load_intent();
    let with_row = expected_v0_snapshot(
        &with_row,
        &intent,
        &std::fs::read(golden_path()).expect("read golden"),
    )
    .expect("snapshot with golden row");
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
    let expected = expected_v0_snapshot(
        &expected_path,
        &load_intent(),
        &std::fs::read(golden_path()).expect("read golden"),
    )
    .expect("expected golden snapshot");
    Connection::open(&actual_path)
        .expect("open candidate")
        .execute(
            "INSERT INTO body_metrics (date, metric, value, unit, source) \
             VALUES ('2026-08-29T09:15:30.123456+01:00', 'weight_kg', 99, 'kg', 'manual')",
            [],
        )
        .expect("induce value drift");

    let differences = compare_snapshots(
        &expected,
        &normalize_database(&actual_path).expect("actual snapshot"),
        &DifferenceAllowList::empty(),
    )
    .expect("value diff");
    assert!(differences.iter().any(|diff| {
        diff.path.starts_with("/tables/body_metrics/rows/0/") && diff.expected != diff.actual
    }));
}

#[test]
fn blob_values_are_normalized_losslessly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "blob.db");
    Connection::open(&database)
        .expect("open candidate")
        .execute_batch("CREATE TABLE blobs (value BLOB); INSERT INTO blobs VALUES (x'00ff10');")
        .expect("insert representative blob");

    let snapshot = normalize_database(&database).expect("normalize blob database");
    assert_eq!(
        snapshot.tables["blobs"].rows,
        vec![vec![CellValue::Blob("00ff10".to_owned())]]
    );
}

#[test]
fn non_utf8_text_is_normalized_losslessly_and_distinct_from_blobs() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "non-utf8.db");
    Connection::open(&database)
        .expect("open candidate")
        .execute_batch(
            "CREATE TABLE byte_values (value); \
             INSERT INTO byte_values VALUES (CAST(x'80ff' AS TEXT)); \
             INSERT INTO byte_values VALUES (x'80ff');",
        )
        .expect("insert representative byte values");

    let snapshot = normalize_database(&database).expect("normalize non-UTF-8 text database");
    assert_eq!(
        snapshot.tables["byte_values"].rows,
        vec![
            vec![CellValue::NonUtf8Text("80ff".to_owned())],
            vec![CellValue::Blob("80ff".to_owned())],
        ]
    );
}

#[test]
fn column_nullability_is_normalized_exactly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "columns.db");
    let snapshot = normalize_database(&database).expect("normalize fixture");
    let columns = &snapshot.tables["body_metrics"].columns;
    let nullability = columns
        .iter()
        .map(|column| (column.name.as_str(), column.not_null))
        .collect::<Vec<_>>();
    assert_eq!(
        nullability,
        vec![
            ("id", false),
            ("date", true),
            ("metric", true),
            ("value", true),
            ("unit", true),
            ("source", true),
        ]
    );
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
    assert_eq!(
        compare_snapshots(
            &expected,
            &actual,
            &DifferenceAllowList::explicit(vec!["/schema_version/child".to_owned()]),
        )
        .expect("non-exact allow-list entry"),
        vec![gym_bot::parity::StateDifference {
            path: "/schema_version".to_owned(),
            expected: Some(serde_json::json!(5)),
            actual: Some(serde_json::json!(6)),
        }]
    );
}
