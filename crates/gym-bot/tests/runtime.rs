//! Owner/dedup integration across the production-neutral runtime bridge.

mod common;

use std::sync::Arc;

use botkit::{SurfaceId, UpdateKey};
use gym_bot::{
    clock::FixedClock,
    command::CommandInput,
    runtime::RuntimeService,
    service::{PreferenceReviewDecision, PreferenceReviewRequest},
    telegram::PreferenceCallbackInput,
};
use rusqlite::Connection;

const NOW: &str = "2026-08-30T10:15:00+01:00";

fn input(actor: &str, update_id: &str, text: &str) -> CommandInput {
    CommandInput {
        actor_id: actor.to_owned(),
        update: UpdateKey::new(SurfaceId::new("telegram").expect("surface"), update_id)
            .expect("key"),
        text: text.to_owned(),
    }
}

#[test]
fn every_runtime_route_is_owner_first_and_restart_deduplicated() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let sidecar = directory.path().join("processed.db");
    let runtime = RuntimeService::new("1001", &database, &sidecar, Arc::new(FixedClock::new(NOW)))
        .expect("runtime");

    assert_eq!(
        runtime
            .handle_message(&input("999", "1", "/weight 80"), 1001, 11)
            .expect("unauthorized"),
        None
    );
    let weight = input("1001", "2", "/weight 80");
    assert!(
        runtime
            .handle_message(&weight, 1001, 12)
            .expect("weight")
            .expect("reply")
            .contains("Logged weight")
    );
    assert_eq!(
        runtime
            .handle_message(&weight, 1001, 12)
            .expect("duplicate"),
        None
    );
    drop(runtime);

    let restarted =
        RuntimeService::new("1001", &database, &sidecar, Arc::new(FixedClock::new(NOW)))
            .expect("restart");
    assert_eq!(
        restarted
            .handle_message(&weight, 1001, 12)
            .expect("restart duplicate"),
        None
    );
    assert_eq!(
        Connection::open(database)
            .expect("database")
            .query_row("SELECT count(*) FROM body_metrics", [], |row| row
                .get::<_, i64>(0))
            .expect("weight count"),
        1
    );
}

#[test]
fn batch_free_text_is_buffered_and_twentieth_entry_fails_closed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let runtime = RuntimeService::new(
        "1001",
        &database,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(NOW)),
    )
    .expect("runtime");
    assert!(
        runtime
            .handle_message(&input("1001", "open", "/batch open"), 700, 1)
            .expect("open")
            .expect("reply")
            .contains("Batch opened")
    );
    for index in 1..20 {
        assert_eq!(
            runtime
                .handle_message(
                    &input("1001", &format!("entry-{index}"), "bench 3x5"),
                    700,
                    index,
                )
                .expect("append"),
            None
        );
    }
    let reply = runtime
        .handle_message(&input("1001", "entry-20", "run 5k"), 700, 20)
        .expect("twentieth")
        .expect("fail closed reply");
    assert!(reply.contains("D23"));
    assert!(reply.contains("20-message batch was kept"));
    assert_eq!(
        Connection::open(database)
            .expect("database")
            .query_row(
                "SELECT count(*) FROM batch_buffer WHERE chat_id=700",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("buffer count"),
        20
    );
}

#[test]
fn preference_callbacks_share_owner_and_update_key_authority() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let connection = Connection::open(&database).expect("database");
    connection
        .execute(
            "INSERT INTO preferences (key,value,confidence,source,active) \
             VALUES ('warmup','short',0.8,'inferred',0)",
            [],
        )
        .expect("proposal");
    let id = connection.last_insert_rowid();
    drop(connection);
    let runtime = RuntimeService::new(
        "1001",
        &database,
        directory.path().join("processed.db"),
        Arc::new(FixedClock::new(NOW)),
    )
    .expect("runtime");
    let callback = |actor: &str, update: &str| PreferenceCallbackInput {
        actor_id: actor.to_owned(),
        update: UpdateKey::new(SurfaceId::new("telegram").expect("surface"), update).expect("key"),
        review: PreferenceReviewRequest {
            preference_id: id,
            decision: PreferenceReviewDecision::Keep,
        },
    };
    assert_eq!(
        runtime
            .handle_preference_callback(&callback("999", "callback-1"))
            .expect("unauthorized"),
        None
    );
    assert_eq!(
        runtime
            .handle_preference_callback(&callback("1001", "callback-2"))
            .expect("review"),
        Some("Preference accepted.".to_owned())
    );
    assert_eq!(
        runtime
            .handle_preference_callback(&callback("1001", "callback-2"))
            .expect("duplicate"),
        None
    );
}
