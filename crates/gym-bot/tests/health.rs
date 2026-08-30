//! Strict, transactional Apple Health import tests.

mod common;

use gym_bot::health::HealthImporter;
use rusqlite::Connection;

#[test]
fn health_import_is_transactional_and_replay_safe() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let importer = HealthImporter::new(&database);
    let payload = br#"{
      "workouts":[{"external_id":"watch-run-1","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1800,"distance_m":5000,"avg_hr":145,"splits":[{"distance_m":1000,"duration_s":350,"avg_hr":140}],"hr_samples":[{"at":"2026-08-30T09:01:00+01:00","bpm":130}]}],
      "metrics":[{"external_id":"resting-1","at":"2026-08-30T07:00:00+01:00","metric":"resting_hr","value":52,"unit":"bpm"}]
    }"#;
    assert_eq!(
        importer.import_json(payload).expect("first import"),
        gym_bot::health::ImportResult {
            inserted: 2,
            duplicates: 0
        }
    );
    assert_eq!(
        importer.import_json(payload).expect("duplicate import"),
        gym_bot::health::ImportResult {
            inserted: 0,
            duplicates: 2
        }
    );
    let connection = Connection::open(database).expect("open result");
    assert_eq!(count(&connection, "sessions"), 1);
    assert_eq!(count(&connection, "effort_splits"), 1);
    assert_eq!(count(&connection, "hr_samples"), 1);
    assert_eq!(count(&connection, "body_metrics"), 1);
    assert_eq!(count(&connection, "external_activities"), 2);
}

#[test]
fn health_import_conservatively_reconciles_matching_manual_cardio() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("open fixture");
    connection.execute_batch(
        "INSERT INTO sessions (started_at,kind,source) VALUES ('2026-08-30T09:05:00+01:00','cardio','manual');
         INSERT INTO movements (name,display_name,modality) VALUES ('run','Run','cardio');
         INSERT INTO session_items (session_id,position,movement_id) VALUES (1,1,1);
         INSERT INTO efforts (session_item_id,position,duration_s,distance_m) VALUES (1,1,1900,4900);",
    ).expect("manual activity");
    drop(connection);
    let payload = br#"{"workouts":[{"external_id":"watch-run","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1800,"distance_m":5000,"avg_hr":145}],"metrics":[]}"#;
    HealthImporter::new(&database)
        .import_json(payload)
        .expect("reconcile");
    let connection = Connection::open(database).expect("open result");
    assert_eq!(count(&connection, "sessions"), 1);
    assert_eq!(
        connection
            .query_row("SELECT source FROM sessions", [], |row| row
                .get::<_, String>(0))
            .expect("source"),
        "apple_health"
    );
    assert_eq!(
        connection
            .query_row("SELECT distance_m FROM efforts", [], |row| row
                .get::<_, f64>(0))
            .expect("distance")
            .to_bits(),
        5000.0_f64.to_bits()
    );
}

#[test]
fn health_nonmatching_manual_activity_creates_a_separate_session() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("open fixture");
    connection.execute_batch(
        "INSERT INTO sessions (started_at,kind,source) VALUES ('2026-08-30T06:00:00+01:00','cardio','manual');
         INSERT INTO movements (name,display_name,modality) VALUES ('run','Run','cardio');
         INSERT INTO session_items (session_id,position,movement_id) VALUES (1,1,1);
         INSERT INTO efforts (session_item_id,position,duration_s,distance_m) VALUES (1,1,1800,1000);",
    ).expect("manual activity");
    drop(connection);
    let payload = br#"{"workouts":[{"external_id":"watch-late","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1800}],"metrics":[]}"#;
    HealthImporter::new(&database)
        .import_json(payload)
        .expect("new session");
    assert_eq!(
        count(&Connection::open(database).expect("result"), "sessions"),
        2
    );
}

#[test]
fn reconciliation_distance_tolerance_and_missing_values_are_independent() {
    for (existing, imported, expected_sessions) in [
        ("NULL", "5000", 1),
        ("5000", "null", 1),
        ("4500", "5000", 1),
        ("4499", "5000", 2),
    ] {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = common::copy_fixture(&directory, "gym.db");
        Connection::open(&database)
            .expect("fixture")
            .execute_batch(&format!(
                "INSERT INTO sessions (started_at,kind,source) VALUES ('2026-08-30T09:00:00+01:00','cardio','manual');
                 INSERT INTO movements (name,display_name,modality) VALUES ('run','Run','cardio');
                 INSERT INTO session_items (session_id,position,movement_id) VALUES (1,1,1);
                 INSERT INTO efforts (session_item_id,position,duration_s,distance_m) VALUES (1,1,1800,{existing});"
            ))
            .expect("manual row");
        let payload = format!(
            r#"{{"workouts":[{{"external_id":"watch","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1800,"distance_m":{imported}}}],"metrics":[]}}"#
        );
        HealthImporter::new(&database)
            .import_json(payload.as_bytes())
            .expect("import");
        assert_eq!(
            count(&Connection::open(database).expect("result"), "sessions"),
            expected_sessions
        );
    }
}

#[test]
fn health_import_accepts_optional_split_and_metric_variants() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let payload = br#"{"workouts":[{"external_id":"interval","started_at":"2026-08-30T09:00:00+01:00","activity":"Cycle","duration_s":600,"splits":[{"distance_m":null,"duration_s":60,"avg_hr":null}]}],"metrics":[{"external_id":"hrv","at":"2026-08-30T07:00:00+01:00","metric":"hrv_ms","value":42,"unit":"ms"},{"external_id":"sleep","at":"2026-08-30T07:00:00+01:00","metric":"sleep_s","value":28800,"unit":"s"},{"external_id":"vo2","at":"2026-08-30T07:00:00+01:00","metric":"vo2max","value":45,"unit":"mL/min/kg"}]}"#;
    let result = HealthImporter::new(&database)
        .import_json(payload)
        .expect("optional variants");
    assert_eq!(result.inserted, 4);
    let connection = Connection::open(database).expect("result");
    assert_eq!(count(&connection, "effort_splits"), 1);
    assert_eq!(count(&connection, "body_metrics"), 3);
}

#[test]
fn health_accepts_zero_distance_and_exact_text_limits() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let external_id = "x".repeat(200);
    let activity = "a".repeat(100);
    let payload = serde_json::to_vec(&serde_json::json!({
        "workouts": [{
            "external_id": external_id,
            "started_at": "2026-08-30T09:00:00+01:00",
            "activity": activity,
            "duration_s": 1,
            "distance_m": 0
        }],
        "metrics": []
    }))
    .expect("payload");

    let result = HealthImporter::new(&database)
        .import_json(&payload)
        .expect("inclusive semantic limits");
    assert_eq!(result.inserted, 1);
    let connection = Connection::open(database).expect("result");
    assert_eq!(count(&connection, "sessions"), 1);
    assert_eq!(
        connection
            .query_row("SELECT distance_m FROM efforts", [], |row| {
                row.get::<_, f64>(0)
            })
            .expect("zero distance")
            .to_bits(),
        0.0_f64.to_bits()
    );
}

#[test]
fn duplicate_ids_within_one_payload_are_counted_not_reinserted() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let payload = br#"{"workouts":[],"metrics":[{"external_id":"same","at":"2026-08-30T07:00:00+01:00","metric":"hrv_ms","value":42,"unit":"ms"},{"external_id":"same","at":"2026-08-30T08:00:00+01:00","metric":"hrv_ms","value":43,"unit":"ms"}]}"#;
    let result = HealthImporter::new(database)
        .import_json(payload)
        .expect("dedup payload");
    assert_eq!(result.inserted, 1);
    assert_eq!(result.duplicates, 1);
}

#[test]
fn malformed_or_late_failure_rolls_back_the_whole_payload() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let importer = HealthImporter::new(&database);
    for payload in [
        br#"{"workouts":[{"external_id":"bad","started_at":"nope","activity":"Run","duration_s":1}],"metrics":[]}"#.as_slice(),
        br#"{"workouts":[],"metrics":[{"external_id":"bad","at":"2026-08-30T09:00:00+01:00","metric":"private_metric","value":1,"unit":"x"}]}"#.as_slice(),
    ] {
        assert!(importer.import_json(payload).is_err());
    }
    let connection = Connection::open(database).expect("open result");
    assert_eq!(count(&connection, "sessions"), 0);
    assert_eq!(count(&connection, "body_metrics"), 0);
    assert_eq!(count(&connection, "external_activities"), 0);
}

#[test]
fn health_validation_rejects_bounds_unknown_fields_and_non_finite_values() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let importer = HealthImporter::new(database);
    for payload in [
        br#"{"unknown":true}"#.as_slice(),
        br#"{"workouts":[{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":0}],"metrics":[]}"#,
        br#"{"workouts":[{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1,"distance_m":-1}],"metrics":[]}"#,
        br#"{"workouts":[{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1,"hr_samples":[{"at":"bad","bpm":1}]}],"metrics":[]}"#,
        br#"{"workouts":[],"metrics":[{"external_id":"x","at":"2026-08-30T09:00:00+01:00","metric":"hrv_ms","value":-1,"unit":"ms"}]}"#,
        br#"{"workouts":[{"external_id":"","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1}],"metrics":[]}"#,
        br#"{"workouts":[{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"","duration_s":1}],"metrics":[]}"#,
        br#"{"workouts":[{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1,"avg_hr":0}],"metrics":[]}"#,
        br#"{"workouts":[{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"Run","duration_s":1,"hr_samples":[{"at":"2026-08-30T09:00:00+01:00","bpm":0}]}],"metrics":[]}"#,
    ] {
        assert!(importer.import_json(payload).is_err());
    }
    assert!(importer.import_json(&vec![b' '; 1_048_577]).is_err());
    let too_many = format!(
        "{{\"workouts\":[],\"metrics\":[{}]}}",
        std::iter::repeat_n(
            r#"{"external_id":"x","at":"2026-08-30T09:00:00+01:00","metric":"hrv_ms","value":1,"unit":"ms"}"#,
            501
        )
        .collect::<Vec<_>>()
        .join(",")
    );
    assert!(importer.import_json(too_many.as_bytes()).is_err());
    let too_many_workouts = format!(
        "{{\"workouts\":[{}],\"metrics\":[]}}",
        std::iter::repeat_n(
            r#"{"external_id":"x","started_at":"2026-08-30T09:00:00+01:00","activity":"run","duration_s":1}"#,
            101
        )
        .collect::<Vec<_>>()
        .join(",")
    );
    assert!(importer.import_json(too_many_workouts.as_bytes()).is_err());
    for count in [100, 500] {
        assert!(count > 0);
    }
}

#[test]
fn health_collection_limits_accept_exact_maxima_and_reject_one_more() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let importer = HealthImporter::new(database);
    for (key, maximum) in [("workouts", 100), ("metrics", 500)] {
        let other = if key == "workouts" {
            "metrics"
        } else {
            "workouts"
        };
        let values = |count| {
            (0..count).map(|index| if key == "workouts" { format!(r#"{{"external_id":"w{index}","started_at":"2026-08-30T09:00:00+01:00","activity":"run","duration_s":1}}"#) } else { format!(r#"{{"external_id":"m{index}","at":"2026-08-30T09:00:00+01:00","metric":"hrv_ms","value":0,"unit":"ms"}}"#) }).collect::<Vec<_>>().join(",")
        };
        let exact = format!("{{\"{key}\":[{}],\"{other}\":[]}}", values(maximum));
        let over = format!("{{\"{key}\":[{}],\"{other}\":[]}}", values(maximum + 1));
        assert!(importer.import_json(exact.as_bytes()).is_ok());
        assert!(importer.import_json(over.as_bytes()).is_err());
    }
}

#[test]
fn health_sample_limits_are_exact() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let importer = HealthImporter::new(database);
    for (field, maximum, sample) in [
        ("splits", 1_000, r#"{"distance_m":1}"#),
        (
            "hr_samples",
            5_000,
            r#"{"at":"2026-08-30T09:00:00+01:00","bpm":1}"#,
        ),
    ] {
        let values = |count| {
            std::iter::repeat_n(sample, count)
                .collect::<Vec<_>>()
                .join(",")
        };
        let payload = |count| {
            format!(
                r#"{{"workouts":[{{"external_id":"{field}-{count}","started_at":"2026-08-30T09:00:00+01:00","activity":"run","duration_s":1,"{field}":[{}]}}],"metrics":[]}}"#,
                values(count)
            )
        };
        assert!(importer.import_json(payload(maximum).as_bytes()).is_ok());
        assert!(
            importer
                .import_json(payload(maximum + 1).as_bytes())
                .is_err()
        );
    }
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count fixture table")
}

#[test]
fn raw_payload_byte_limit_is_exact() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let importer = HealthImporter::new(database);
    assert!(matches!(
        importer.import_json(&vec![b' '; 1_048_576]),
        Err(gym_bot::health::HealthError::Json(_))
    ));
    assert!(matches!(
        importer.import_json(&vec![b' '; 1_048_577]),
        Err(gym_bot::health::HealthError::PayloadTooLarge)
    ));
}
