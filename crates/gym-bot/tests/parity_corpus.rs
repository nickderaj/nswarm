//! Multi-intent full SQLite-state parity corpus.

mod common;

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

fn load_corpus() -> Corpus {
    serde_json::from_slice(
        &std::fs::read(fixture_root().join("parity-corpus.json")).expect("read corpus"),
    )
    .expect("parse corpus")
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym")
}
