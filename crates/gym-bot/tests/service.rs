//! Deterministic gym service integration tests.

mod common;

use std::sync::Arc;

use gym_bot::{
    clock::FixedClock,
    service::{GymService, ServiceRequest},
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
