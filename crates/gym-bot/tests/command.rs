//! Transport-neutral `/weight` command integration and property tests.

mod common;

use std::sync::Arc;

use botkit::{SurfaceId, UpdateKey};
use gym_bot::{
    clock::{Clock, FixedClock, SystemClock, timestamp_in_timezone},
    command::{
        CommandError, CommandInput, CommandResult, CommandService, IgnoreReason, WeightParseError,
        parse_weight_command,
    },
    database::{DatabaseError, V0_GYM_SCHEMA_VERSION, open_existing, validate_existing},
};
use proptest::prelude::*;
use rusqlite::Connection;

const FIXED_TIME: &str = "2026-08-29T08:15:30+00:00";

fn input(actor: &str, surface: &str, external_id: &str, text: &str) -> CommandInput {
    CommandInput {
        actor_id: actor.to_owned(),
        update: UpdateKey::new(
            SurfaceId::new(surface).expect("valid test surface"),
            external_id,
        )
        .expect("valid test update"),
        text: text.to_owned(),
        conversation_id: "1001".to_owned(),
    }
}

#[test]
fn weight_command_writes_exact_v0_intent_and_response() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = CommandService::new(
        "owner",
        &database,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .expect("command service");
    assert_eq!(service.database_path(), database);

    let result = service
        .handle(&input("owner", "telegram", "41", "/weight 82.5kg"))
        .expect("command succeeds");

    assert_eq!(
        result,
        CommandResult::Reply("✅ Logged weight: 82.5 kg".to_owned())
    );
    let connection = open_existing(&database).expect("open written fixture");
    let row = connection
        .query_row(
            "SELECT id, date, metric, value, unit, source FROM body_metrics",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .expect("body metric row");
    assert_eq!(
        row,
        (
            1,
            FIXED_TIME.to_owned(),
            "weight_kg".to_owned(),
            82.5,
            "kg".to_owned(),
            "manual".to_owned()
        )
    );
}

#[test]
fn weight_success_response_matches_python_general_formatting() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = CommandService::new(
        "owner",
        &database,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .expect("command service");
    for (index, (value, expected)) in [
        ("1234567", "1.23457e+06"),
        ("82.123456789", "82.1235"),
        ("0.00001", "1e-05"),
        ("100000", "100000"),
        ("493159.654299074", "493160"),
        ("983299.6961915712", "983300"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            service
                .handle(&input(
                    "owner",
                    "telegram",
                    &format!("format-{index}"),
                    &format!("/weight {value}"),
                ))
                .expect("formatted command"),
            CommandResult::Reply(format!("✅ Logged weight: {expected} kg"))
        );
    }
}

#[test]
fn production_clock_returns_rfc3339_in_the_configured_zone() {
    let timestamp = SystemClock::new("Europe/London")
        .expect("installed IANA zone")
        .now_iso8601();
    assert_eq!(timestamp.matches('.').count(), 1);
    chrono::DateTime::parse_from_rfc3339(&timestamp).expect("production clock is RFC-3339");
    assert_eq!(
        timestamp_in_timezone("2026-08-29T08:15:30.123456Z", "Europe/London")
            .expect("fixed zoned timestamp"),
        "2026-08-29T09:15:30.123456+01:00"
    );
    assert_eq!(
        timestamp_in_timezone("2026-01-29T08:15:30Z", "Europe/London")
            .expect("fixed winter timestamp"),
        "2026-01-29T08:15:30+00:00"
    );
}

#[test]
fn owner_rejection_does_not_consume_update_or_write_state() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = CommandService::new(
        "owner",
        &database,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .expect("command service");
    let rejected = service
        .handle(&input("intruder", "telegram", "42", "/weight 80"))
        .expect("owner rejection is ordinary");
    assert_eq!(rejected, CommandResult::Ignored(IgnoreReason::NotOwner));

    let accepted = service
        .handle(&input("owner", "telegram", "42", "/weight 80"))
        .expect("same key remains available to owner");
    assert_eq!(
        accepted,
        CommandResult::Reply("✅ Logged weight: 80 kg".to_owned())
    );
    assert_eq!(metric_count(&database), 1);
}

#[test]
fn duplicate_identity_is_the_generic_surface_external_id_pair() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let processed = directory.path().join("processed.db");
    let service = CommandService::new(
        "owner",
        &database,
        &processed,
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .expect("command service");
    let telegram = input("owner", "telegram", "43", "/weight 81");
    assert!(matches!(
        service.handle(&telegram),
        Ok(CommandResult::Reply(_))
    ));
    assert_eq!(
        service.handle(&telegram).expect("duplicate is ordinary"),
        CommandResult::Ignored(IgnoreReason::DuplicateUpdate)
    );
    assert!(matches!(
        service.handle(&input("owner", "web", "43", "/weight 81")),
        Ok(CommandResult::Reply(_))
    ));
    assert_eq!(metric_count(&database), 2);

    drop(service);
    let restarted = CommandService::new(
        "owner",
        &database,
        &processed,
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .expect("restarted command service");
    assert_eq!(
        restarted
            .handle(&telegram)
            .expect("restart duplicate is ordinary"),
        CommandResult::Ignored(IgnoreReason::DuplicateUpdate)
    );
    assert_eq!(metric_count(&database), 2);
    let processed_connection = Connection::open(&processed).expect("open processed sidecar");
    let durable_keys = processed_connection
        .prepare("SELECT surface, external_id FROM processed_updates ORDER BY surface")
        .expect("prepare processed query")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query processed keys")
        .collect::<Result<Vec<_>, _>>()
        .expect("read processed keys");
    assert_eq!(
        durable_keys,
        vec![
            ("telegram".to_owned(), "43".to_owned()),
            ("web".to_owned(), "43".to_owned()),
        ]
    );
}

#[test]
fn blank_malformed_non_finite_and_non_positive_weights_are_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = CommandService::new(
        "owner",
        &database,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(FIXED_TIME)),
    )
    .expect("command service");
    let invalid = [
        "",
        "/weight",
        "/weight kg",
        "/weight nope",
        "/weight NaN",
        "/weight inf",
        "/weight -inf",
        "/weight 0",
        "/weight -0.1",
        "/weight 80 extra",
        "/Weight 80",
    ];
    for (index, text) in invalid.into_iter().enumerate() {
        assert_eq!(
            service
                .handle(&input("owner", "telegram", &index.to_string(), text))
                .expect("invalid command has usage reply"),
            CommandResult::Reply("Usage: /weight <kg>".to_owned()),
            "input {text:?}"
        );
    }
    assert_eq!(metric_count(&database), 0);
}

#[test]
fn existing_fixture_open_is_non_migrating_and_fail_closed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = open_existing(&database).expect("v0 fixture opens");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version");
    assert_eq!(version, V0_GYM_SCHEMA_VERSION);
    drop(connection);

    let drifted = Connection::open(&database).expect("open to induce schema drift");
    drifted
        .pragma_update(None, "user_version", V0_GYM_SCHEMA_VERSION + 1)
        .expect("drift version");
    drop(drifted);
    assert!(matches!(
        open_existing(&database),
        Err(DatabaseError::SchemaVersion {
            expected: V0_GYM_SCHEMA_VERSION,
            actual
        }) if actual == V0_GYM_SCHEMA_VERSION + 1
    ));
    assert!(matches!(
        open_existing(&directory.path().join("missing.db")),
        Err(DatabaseError::Sqlite(_))
    ));
    assert!(matches!(
        CommandService::new(
            "owner",
            &database,
            &database,
            Arc::new(FixedClock::new(FIXED_TIME)),
        ),
        Err(CommandError::IdempotencyPathAliasesGymDatabase)
    ));

    let invalid_foreign_key = common::copy_fixture(&directory, "invalid-fk.db");
    Connection::open(&invalid_foreign_key)
        .expect("open fixture to induce foreign-key drift")
        .execute_batch(
            "PRAGMA foreign_keys=OFF; \
             INSERT INTO session_items (session_id, position, movement_id) VALUES (99, 1, 88);",
        )
        .expect("insert invalid foreign keys while enforcement is off");
    assert!(matches!(
        validate_existing(&invalid_foreign_key),
        Err(DatabaseError::ForeignKeyViolations(2))
    ));
    assert!(
        open_existing(&invalid_foreign_key).is_ok(),
        "per-command open performs only the cheap schema check"
    );
}

#[test]
fn idempotency_sidecar_rejects_filesystem_aliases_of_the_gym_database() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let symlink = directory.path().join("gym-symlink.db");
    std::os::unix::fs::symlink(&database, &symlink).expect("create database symlink");
    let hard_link = directory.path().join("gym-hard-link.db");
    std::fs::hard_link(&database, &hard_link).expect("create database hard link");

    for alias in [directory.path().join("./gym.db"), symlink, hard_link] {
        assert!(matches!(
            CommandService::new(
                "owner",
                &database,
                alias,
                Arc::new(FixedClock::new(FIXED_TIME)),
            ),
            Err(CommandError::IdempotencyPathAliasesGymDatabase)
        ));
    }
    open_existing(&database).expect("alias rejection leaves frozen schema intact");
}

fn metric_count(database: &std::path::Path) -> i64 {
    open_existing(database)
        .expect("open fixture")
        .query_row("SELECT count(*) FROM body_metrics", [], |row| row.get(0))
        .expect("metric count")
}

proptest! {
    #[test]
    fn every_positive_finite_decimal_round_trips(value in 0.001_f64..1000.0) {
        let text = format!("/weight {value}kg");
        let parsed = parse_weight_command(&text).expect("positive finite property input");
        prop_assert_eq!(parsed.kilograms.to_bits(), value.to_bits());
    }

    #[test]
    fn arbitrary_text_never_yields_an_invalid_weight(text in ".{0,256}") {
        if let Ok(parsed) = parse_weight_command(&text) {
            prop_assert!(parsed.kilograms.is_finite());
            prop_assert!(parsed.kilograms > 0.0);
        } else {
            prop_assert_eq!(parse_weight_command(&text), Err(WeightParseError::Usage));
        }
    }
}
