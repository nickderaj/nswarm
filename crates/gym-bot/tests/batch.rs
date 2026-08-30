//! Durable batch claim, retry, and restart tests.

mod common;

use chrono::DateTime;
use gym_bot::batch::BatchService;

fn at(value: &str) -> DateTime<chrono::FixedOffset> {
    DateTime::parse_from_rfc3339(value).expect("test timestamp")
}

#[test]
fn batch_survives_restart_deduplicates_and_becomes_due() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    BatchService::new(&database)
        .open(1001, at("2026-08-30T09:00:00+01:00"))
        .expect("open");
    assert!(
        BatchService::new(&database)
            .append(1001, 7, "bench 3x8", at("2026-08-30T09:01:00+01:00"))
            .expect("append")
    );
    assert!(
        !BatchService::new(&database)
            .append(1001, 7, "duplicate", at("2026-08-30T09:02:00+01:00"))
            .expect("deduplicate")
    );
    assert!(
        BatchService::new(&database)
            .due(at("2026-08-30T20:59:59+01:00"))
            .expect("early")
            .is_empty()
    );
    assert_eq!(
        BatchService::new(&database)
            .due(at("2026-08-30T21:00:00+01:00"))
            .expect("due"),
        [1001]
    );
}

#[test]
fn failed_processing_keeps_snapshot_and_completion_preserves_concurrent_append() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let batch = BatchService::new(&database);
    batch
        .open(1001, at("2026-08-30T09:00:00+01:00"))
        .expect("open");
    batch
        .append(1001, 1, "first", at("2026-08-30T09:01:00+01:00"))
        .expect("first");
    let snapshot = batch.snapshot(1001).expect("snapshot");
    assert_eq!(batch.snapshot(1001).expect("retry unchanged"), snapshot);
    batch
        .append(1001, 2, "later", at("2026-08-30T09:02:00+01:00"))
        .expect("later");
    batch
        .complete(1001, &snapshot)
        .expect("complete first snapshot");
    assert_eq!(
        batch.snapshot(1001).expect("later survives")[0].text,
        "later"
    );
    assert_eq!(batch.cancel(1001).expect("cancel"), 1);
    assert!(batch.snapshot(1001).expect("empty").is_empty());
}
