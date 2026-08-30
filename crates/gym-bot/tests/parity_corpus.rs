//! Multi-intent full SQLite-state parity corpus.

mod common;

use std::process::Command;
use std::{path::PathBuf, sync::Arc};

use gym_bot::{
    clock::FixedClock,
    parity::{DifferenceAllowList, compare_snapshots, normalize_database},
    service::{GymService, ServiceRequest},
};
use rusqlite::Connection;
use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    schema_version: u8,
    fixed_time: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    command: String,
    golden: PathBuf,
}

#[test]
fn every_ported_mutation_matches_independent_v0_golden_state() {
    let corpus = load_corpus();
    assert_eq!(corpus.schema_version, 1);
    let mut ids = std::collections::BTreeSet::new();
    for case in &corpus.cases {
        assert!(ids.insert(&case.id), "duplicate parity id: {}", case.id);
        let directory = tempfile::tempdir().expect("tempdir");
        let expected = common::copy_fixture(&directory, "expected.db");
        let actual = common::copy_fixture(&directory, "actual.db");
        Connection::open(&expected)
            .expect("expected database")
            .execute_batch(
                &std::fs::read_to_string(fixture_root().join(&case.golden))
                    .expect("read v0 golden"),
            )
            .expect("apply v0 golden");
        GymService::new(&actual, Arc::new(FixedClock::new(&corpus.fixed_time)))
            .handle(&ServiceRequest {
                conversation_id: "1001".to_owned(),
                text: case.command.clone(),
            })
            .expect("apply v1 intent");
        if case.id == "record-preference" {
            for path in [&expected, &actual] {
                Connection::open(path)
                    .expect("normalize timestamp")
                    .execute(
                        "UPDATE preferences SET created_at='2026-08-30 09:15:00', \
                         updated_at='2026-08-30 09:15:00', reviewed_at='2026-08-30 09:15:00'",
                        [],
                    )
                    .expect("set allowed deterministic instant");
            }
        }
        let differences = compare_snapshots(
            &normalize_database(&expected).expect("normalize expected"),
            &normalize_database(&actual).expect("normalize actual"),
            &DifferenceAllowList::empty(),
        )
        .expect("compare snapshots");
        assert!(differences.is_empty(), "{}: {differences:#?}", case.id);
    }
}

#[test]
fn deliberate_corpus_mismatch_is_detected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected = common::copy_fixture(&directory, "expected.db");
    let actual = common::copy_fixture(&directory, "actual.db");
    Connection::open(&actual)
        .expect("actual database")
        .execute("PRAGMA user_version=4", [])
        .expect("drift schema version");
    let differences = compare_snapshots(
        &normalize_database(&expected).expect("normalize expected"),
        &normalize_database(&actual).expect("normalize actual"),
        &DifferenceAllowList::empty(),
    )
    .expect("compare mismatch");
    assert!(
        differences
            .iter()
            .any(|difference| difference.path == "/schema_version")
    );
}

#[test]
fn normalized_comparison_cli_reports_equal_different_and_usage() {
    let directory = tempfile::tempdir().expect("tempdir");
    let expected = common::copy_fixture(&directory, "expected.db");
    let actual = common::copy_fixture(&directory, "actual.db");
    let program = env!("CARGO_BIN_EXE_gym-db-compare");
    let equal = Command::new(program)
        .arg(&expected)
        .arg(&actual)
        .output()
        .expect("compare equal");
    assert!(equal.status.success());
    assert_eq!(String::from_utf8_lossy(&equal.stdout), "equal\n");
    Connection::open(&actual)
        .expect("actual")
        .execute("PRAGMA user_version=4", [])
        .expect("drift");
    let different = Command::new(program)
        .arg(&expected)
        .arg(&actual)
        .output()
        .expect("compare drift");
    assert_eq!(different.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&different.stdout).contains("schema_version"));
    let usage = Command::new(program).output().expect("compare usage");
    assert_eq!(usage.status.code(), Some(2));
    let missing = Command::new(program)
        .arg(directory.path().join("missing.db"))
        .arg(&actual)
        .output()
        .expect("compare missing");
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("comparison failed"));
}

fn load_corpus() -> Corpus {
    serde_json::from_slice(
        &std::fs::read(fixture_root().join("parity-corpus.json")).expect("read corpus"),
    )
    .expect("parse corpus")
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym")
}
