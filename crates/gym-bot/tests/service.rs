//! Deterministic gym service integration tests.

mod common;

use std::sync::Arc;

use gym_bot::{
    clock::FixedClock,
    service::{
        GymService, PreferenceReviewDecision, PreferenceReviewRequest, ServiceError, ServiceRequest,
    },
};
use rusqlite::Connection;

const NOW: &str = "2026-08-30T10:15:00+01:00";

fn request(text: &str) -> ServiceRequest {
    ServiceRequest {
        conversation_id: "1001".to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn strength_and_cardio_commands_preserve_the_frozen_ledger_shape() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));

    assert_eq!(
        service
            .handle(&request("/gym DB Rows 3x8 60kg @7"))
            .expect("strength"),
        "Logged DB Rows — 3x8 @ 60kg"
    );
    assert_eq!(
        service
            .handle(&request("/cardio easy run 30 5"))
            .expect("cardio"),
        "Logged easy run: 30 min, 5 km"
    );

    let connection = Connection::open(database).expect("open result");
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .expect("sessions"),
        2
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM efforts", [], |row| row
                .get::<_, i64>(0))
            .expect("efforts"),
        4
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT name FROM movements WHERE modality='strength'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .expect("canonical movement"),
        "dumbbell row"
    );
}

#[test]
fn deterministic_reads_preferences_and_rating_work_without_an_agent() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("open fixture");
    connection
        .execute(
            "INSERT INTO workout_plans (focus, plan_json, rationale) VALUES ('legs', '{}', 'test')",
            [],
        )
        .expect("plan");
    connection
        .execute(
            "INSERT INTO model_calls (purpose, model, prompt_tokens, completion_tokens, ok) \
             VALUES ('plan', 'fixture', 10, 5, 1)",
            [],
        )
        .expect("usage");
    drop(connection);
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));

    assert!(
        service
            .handle(&request("/plans"))
            .expect("plans")
            .contains("#1")
    );
    assert_eq!(
        service.handle(&request("/rate 4 useful")).expect("rate"),
        "Thanks — plan feedback saved."
    );
    assert_eq!(
        service
            .handle(&request("/preference warmup short"))
            .expect("preference"),
        "Preference saved."
    );
    assert_eq!(
        service.handle(&request("/cost")).expect("cost"),
        "Model usage: 1 calls · 10 input · 5 output tokens"
    );
    assert_eq!(
        service.handle(&request("coach me")).expect("blocked"),
        "Agent-dependent gym behavior is unavailable while architecture decision D23 is unresolved."
    );
}

#[test]
fn malformed_commands_do_not_write_partial_rows() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    for text in [
        "/gym bench 0x8 60kg",
        "/gym bench 3x8 -1kg",
        "/gym bench 3x8 @11",
        "/cardio run nope",
        "/cardio run 30 -2",
        "/gym 3x8",
        "/gym bench 51x8",
        "/gym bench 3x1001",
        "/gym bench 3x8 nope",
        "/gym bench 3x8 @nope",
        "/cardio easy run 30 nope",
    ] {
        assert!(
            service
                .handle(&request(text))
                .expect("bounded reply")
                .starts_with("Usage:")
        );
    }
    let connection = Connection::open(database).expect("open result");
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .expect("sessions"),
        0
    );
}

#[test]
fn populated_plan_and_optional_command_paths_are_rendered() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("fixture");
    connection.execute("INSERT INTO workout_plans (created_at,focus,plan_json,rationale,status,rating) VALUES ('2026-08-30','legs','{}','why','completed',5)", []).expect("plan");
    connection.execute("INSERT INTO external_activities (source,external_id,payload,imported_at) VALUES ('apple_health','x','{}','2026-08-30')", []).expect("health");
    drop(connection);
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    assert!(
        service
            .handle(&request("/plan 1"))
            .expect("plan")
            .contains("Plan #1 · legs")
    );
    assert!(
        service
            .handle(&request("/plans 1"))
            .expect("plans")
            .contains("rated 5")
    );
    assert!(
        service
            .handle(&request("/sync"))
            .expect("sync")
            .contains("2026-08-30")
    );
    assert_eq!(
        service
            .handle(&request("/preference key value words"))
            .expect("preference"),
        "Preference saved."
    );
    assert_eq!(
        service.handle(&request("/run jog 10")).expect("run alias"),
        "Logged jog: 10 min"
    );
    assert_eq!(
        service
            .handle(&request("/cardio row 15 0"))
            .expect("zero distance"),
        "Logged row: 15 min, 0 km"
    );
    assert_eq!(
        service.handle(&request("/gym OHP 1x3")).expect("alias"),
        "Logged OHP — 1x3"
    );
    assert_eq!(
        service.handle(&request("/gym flies 1x10")).expect("plural"),
        "Logged flies — 1x10"
    );
}

#[test]
fn service_fails_closed_when_database_disappears() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    std::fs::remove_file(database).expect("remove database");
    assert!(service.handle(&request("/weight")).is_err());
}

#[test]
fn new_deterministic_writes_propagate_storage_failures() {
    for (trigger, action) in [
        (
            "CREATE TRIGGER fail_batch BEFORE INSERT ON batch_state BEGIN SELECT RAISE(ABORT,'fixture'); END",
            "/batch open",
        ),
        (
            "CREATE TRIGGER fail_export BEFORE UPDATE ON preferences BEGIN SELECT RAISE(ABORT,'fixture'); END",
            "callback",
        ),
    ] {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = common::copy_fixture(&directory, "gym.db");
        let connection = Connection::open(&database).expect("fixture");
        connection.execute_batch(trigger).expect("failure trigger");
        if action == "callback" {
            connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active) VALUES ('fixture','value',0.8,'inferred','fixture',0)", []).expect("proposal");
        }
        drop(connection);
        let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
        let result = if action == "callback" {
            service.review_preference(PreferenceReviewRequest {
                preference_id: 1,
                decision: PreferenceReviewDecision::Keep,
            })
        } else {
            service.handle(&request(action))
        };
        assert!(matches!(result, Err(ServiceError::Sqlite(_))), "{action}");
    }
}

#[test]
fn every_deterministic_read_and_usage_path_is_exercised() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    for (text, expected) in [
        ("/weight", "No weights logged yet."),
        ("/gym", "No strength workouts logged yet."),
        ("/gym bench", "No bench logged yet."),
        ("/cardio", "Usage:"),
        ("/rate", "Usage:"),
        ("/rate 4", "No plan to rate yet."),
        ("/plans", "No plans yet."),
        ("/plan 99", "No plan #99."),
        ("/plan", "Agent-dependent"),
        ("/adherence", "No plans to compare yet."),
        ("/sync", "Apple Health: 0 records"),
        ("/preference", "Usage:"),
        ("/help", "/gym"),
        ("/weight bad", "Usage:"),
        ("/plans bad", "No plans yet."),
        ("/preference key", "Usage:"),
    ] {
        assert!(
            service
                .handle(&request(text))
                .expect("deterministic path")
                .contains(expected),
            "{text} did not contain {expected}"
        );
    }
    service.handle(&request("/weight 80")).expect("weight");
    service
        .handle(&request("/gym bench 2x5 50 @8"))
        .expect("strength");
    service
        .handle(&request("/gym dumbbells 1x1"))
        .expect("plural alias");
    service
        .handle(&request("/gym barbell curls 1x2 10kg"))
        .expect("weight assignment");
    service
        .handle(&request("/cardio bike 20"))
        .expect("no distance");
    assert!(
        service
            .handle(&request("/weight"))
            .expect("weight history")
            .contains("80 kg")
    );
    assert!(
        service
            .handle(&request("/gym bench"))
            .expect("strength history")
            .contains("bench 5 reps")
    );
}

#[test]
fn canonical_aliases_and_cardio_token_positions_are_exact() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    for (input, stored) in [
        ("BB curl", "barbell curl"),
        ("KB swings", "kettlebell swing"),
        ("OHP", "overhead press"),
        ("RDL", "romanian deadlift"),
        ("deads", "deadlift"),
        ("flyes", "fly"),
        ("calves", "calf"),
        ("carries", "carry"),
    ] {
        service
            .handle(&request(&format!("/gym {input} 1x1")))
            .expect("alias command");
        let count: i64 = Connection::open(&database)
            .expect("alias database")
            .query_row(
                "SELECT count(*) FROM movements WHERE name=?1",
                [stored],
                |row| row.get(0),
            )
            .expect("alias count");
        assert_eq!(count, 1, "{input} must normalize to {stored}");
    }
    assert_eq!(
        service
            .handle(&request("/cardio bike ride 12 3"))
            .expect("distance parse"),
        "Logged bike ride: 12 min, 3 km"
    );
}

#[test]
fn batch_commands_manage_durable_state_without_attempting_agent_extraction() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));

    assert_eq!(
        service.handle(&request("/batch status")).expect("status"),
        "Batch: 0 messages"
    );
    assert!(
        service
            .handle(&request("/batch"))
            .expect("open")
            .starts_with("Batch opened.")
    );
    let connection = Connection::open(&database).expect("batch fixture");
    assert_eq!(
        connection
            .query_row(
                "SELECT opened_at || '|' || auto_flush_at FROM batch_state WHERE chat_id=1001",
                [],
                |row| row.get::<_, String>(0)
            )
            .expect("batch deadline"),
        "2026-08-30T10:15:00+01:00|2026-08-30T22:15:00+01:00"
    );
    connection.execute("INSERT INTO batch_buffer (chat_id,message_id,text,sent_at) VALUES (1001,7,'bench notes','2026-08-30T10:16:00+01:00')", []).expect("buffer");
    drop(connection);
    assert_eq!(
        service.handle(&request("/batch status")).expect("status"),
        "Batch: 1 messages since 2026-08-30 10:16"
    );
    for command in ["/batch", "/batch flush", "/batch retry"] {
        assert!(
            service
                .handle(&request(command))
                .expect("blocked extraction")
                .contains("buffer was kept")
        );
    }
    assert_eq!(
        Connection::open(&database)
            .expect("buffer survives")
            .query_row("SELECT count(*) FROM batch_buffer", [], |row| row
                .get::<_, i64>(0))
            .expect("buffer count"),
        1
    );
    assert_eq!(
        service.handle(&request("/batch cancel")).expect("cancel"),
        "Cancelled batch with 1 buffered messages."
    );
    assert_eq!(
        service.handle(&request("/batch flush")).expect("empty"),
        "No active batch."
    );
}

#[test]
fn batch_usage_and_non_telegram_identity_are_honest() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    for command in ["/batch unknown", "/batch status extra"] {
        assert_eq!(
            service.handle(&request(command)).expect("usage"),
            "Usage: /batch [open|status|flush|cancel|retry]"
        );
    }
    assert_eq!(
        service
            .handle(&ServiceRequest {
                conversation_id: "web-conversation".to_owned(),
                text: "/batch open".to_owned(),
            })
            .expect("non-numeric identity"),
        "Batch commands require a numeric conversation id."
    );
    assert!(
        service
            .handle(&request("/batch open"))
            .expect("open")
            .starts_with("Batch opened.")
    );
    assert_eq!(
        service.handle(&request("/batch")).expect("empty toggle"),
        "No active batch."
    );
    assert!(
        service
            .handle(&request("/batch open"))
            .expect("refresh")
            .starts_with("Batch opened.")
    );
}

#[test]
fn adherence_compares_stored_plan_items_with_strength_on_the_planned_day() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("fixture");
    connection.execute(
        "INSERT INTO workout_plans (created_at,for_date,focus,plan_json,rationale) \
         VALUES ('2026-08-29','2026-08-30','push',?1,'test')",
        [r#"{"focus":"push","rationale":"test","items":[{"exercise":"bench press","sets":[{"reps":8}]},{"exercise":"overhead press","sets":[{"reps":8}]}]}"#],
    ).expect("plan");
    drop(connection);
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    service
        .handle(&request("/gym bench press 3x8 60kg"))
        .expect("strength");

    assert_eq!(
        service.handle(&request("/adherence")).expect("adherence"),
        "#1 · 2026-08-30 · 1/2 exercises"
    );
    assert_eq!(
        service.handle(&request("/adherence nope")).expect("usage"),
        "Usage: /adherence [number of plans]"
    );
    assert_eq!(
        service
            .handle(&request("/adherence 1 extra"))
            .expect("usage"),
        "Usage: /adherence [number of plans]"
    );
}

#[test]
fn adherence_default_and_explicit_limits_match_v0_boundaries() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let mut connection = Connection::open(&database).expect("fixture");
    let transaction = connection.transaction().expect("transaction");
    for id in 1..=21 {
        transaction
            .execute(
                "INSERT INTO workout_plans (created_at,focus,plan_json,rationale) \
                 VALUES (?1,'fixture',?2,'test')",
                [
                    format!("2026-08-{id:02}"),
                    r#"{"focus":"fixture","rationale":"test","items":[{"exercise":"bench","sets":[{"reps":1}]}]}"#.to_owned(),
                ],
            )
            .expect("plan");
    }
    transaction.commit().expect("commit");
    drop(connection);
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));

    assert_eq!(
        service
            .handle(&request("/adherence"))
            .expect("default")
            .lines()
            .count(),
        5
    );
    assert_eq!(
        service
            .handle(&request("/adherence 0"))
            .expect("lower clamp")
            .lines()
            .count(),
        1
    );
    assert_eq!(
        service
            .handle(&request("/adherence 21"))
            .expect("upper clamp")
            .lines()
            .count(),
        20
    );
    assert!(
        service
            .handle(&request("/adherence 1"))
            .expect("latest")
            .starts_with("#21 · 2026-08-21")
    );
}

#[test]
fn export_writes_the_frozen_v0_csv_shape_and_escapes_notes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    service
        .handle(&request("/gym DB Rows 1x8 60kg @7"))
        .expect("strength");
    Connection::open(&database)
        .expect("fixture")
        .execute("UPDATE efforts SET notes='steady, \"clean\"'", [])
        .expect("notes");

    let reply = service.handle(&request("/export")).expect("export");
    let destination = directory.path().join("exports/efforts.csv");
    assert_eq!(
        reply,
        format!("Exported 1 efforts to {}", destination.display())
    );
    assert_eq!(
        std::fs::read_to_string(destination).expect("csv"),
        "started_at,movement,position,reps,weight_kg,duration_s,distance_m,rpe,notes\r\n2026-08-30T10:15:00+01:00,dumbbell row,1,8,60.0,,,7,\"steady, \"\"clean\"\"\"\r\n"
    );
}

#[test]
fn preference_keep_and_reject_callbacks_are_one_shot() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("fixture");
    connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active) VALUES ('tempo','slow',0.8,'inferred','fixture',0)", []).expect("keep proposal");
    connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active) VALUES ('warmup','long',0.7,'inferred','fixture',0)", []).expect("reject proposal");
    connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active) VALUES ('stated','unchanged',1.0,'stated','fixture',0)", []).expect("stated row");
    connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active) VALUES ('active','unchanged',0.7,'inferred','fixture',1)", []).expect("active inference");
    connection.execute("INSERT INTO preferences (key,value,confidence,source,evidence,active,reviewed_at) VALUES ('reviewed','unchanged',0.7,'inferred','fixture',0,'2026-08-29')", []).expect("reviewed inference");
    drop(connection);
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));

    assert_eq!(
        service
            .review_preference(PreferenceReviewRequest {
                preference_id: 1,
                decision: PreferenceReviewDecision::Keep,
            })
            .expect("keep"),
        "Preference accepted."
    );
    assert_eq!(
        service
            .review_preference(PreferenceReviewRequest {
                preference_id: 2,
                decision: PreferenceReviewDecision::Reject,
            })
            .expect("reject"),
        "Preference rejected."
    );
    assert_eq!(
        service
            .review_preference(PreferenceReviewRequest {
                preference_id: 1,
                decision: PreferenceReviewDecision::Reject,
            })
            .expect("duplicate"),
        "This preference was already reviewed."
    );
    for preference_id in 3..=5 {
        assert_eq!(
            service
                .review_preference(PreferenceReviewRequest {
                    preference_id,
                    decision: PreferenceReviewDecision::Keep,
                })
                .expect("ineligible preference"),
            "This preference was already reviewed."
        );
    }
    let rows = Connection::open(database)
        .expect("result")
        .prepare("SELECT active,reviewed_at FROM preferences ORDER BY id")
        .expect("statement")
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(
        rows,
        vec![
            (1, NOW.to_owned()),
            (0, NOW.to_owned()),
            (0, String::new()),
            (1, String::new()),
            (0, "2026-08-29".to_owned()),
        ]
    );
}

#[test]
fn unknown_commands_and_unported_archive_import_do_not_claim_d23() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    assert_eq!(
        service.handle(&request("/wieght 80")).expect("typo"),
        "Unknown command. Use /help to see supported gym commands."
    );
    assert_eq!(
        service
            .handle(&request("/import_zip export.zip"))
            .expect("archive"),
        "Apple Health archive import is unavailable through this deterministic service."
    );
    assert!(
        service
            .handle(&request("build me a session"))
            .expect("free text")
            .contains("D23")
    );
    assert!(
        service
            .handle(&request("/plan legs"))
            .expect("plan generation")
            .contains("D23")
    );
}

#[test]
fn invalid_plan_clock_and_export_destination_fail_closed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("fixture");
    connection
        .execute(
            "INSERT INTO workout_plans (plan_json) VALUES ('not-json')",
            [],
        )
        .expect("invalid JSON");
    drop(connection);
    let service = GymService::new(&database, Arc::new(FixedClock::new(NOW)));
    assert!(matches!(
        service.handle(&request("/adherence")),
        Err(ServiceError::Json(_))
    ));
    Connection::open(&database)
        .expect("fixture")
        .execute("UPDATE workout_plans SET plan_json='{}'", [])
        .expect("missing items");
    assert!(matches!(
        service.handle(&request("/adherence")),
        Err(ServiceError::InvalidPlanJson)
    ));
    for items in [
        serde_json::json!([]),
        serde_json::json!(
            (0..21)
                .map(|_| serde_json::json!({"exercise": "bench"}))
                .collect::<Vec<_>>()
        ),
    ] {
        Connection::open(&database)
            .expect("fixture")
            .execute(
                "UPDATE workout_plans SET plan_json=?1",
                [serde_json::json!({"items": items}).to_string()],
            )
            .expect("invalid item count");
        assert!(matches!(
            service.handle(&request("/adherence")),
            Err(ServiceError::InvalidPlanJson)
        ));
    }

    let invalid_clock = GymService::new(&database, Arc::new(FixedClock::new("not-a-time")));
    assert!(matches!(
        invalid_clock.handle(&request("/batch open")),
        Err(ServiceError::Time(_))
    ));

    std::fs::write(directory.path().join("exports"), "not a directory").expect("blocking file");
    assert!(matches!(
        service.handle(&request("/export")),
        Err(ServiceError::Io(_))
    ));
}
