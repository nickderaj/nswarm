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
