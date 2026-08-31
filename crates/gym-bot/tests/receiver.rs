//! Network-free Health receiver contract tests.

mod common;

use gym_bot::{
    health::HealthImporter,
    receiver::{HealthReceiver, HealthRequest},
};

#[test]
fn receiver_requires_bearer_and_returns_bounded_results() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let receiver = HealthReceiver::new("fixture-secret", HealthImporter::new(database));
    let payload = br#"{"workouts":[],"metrics":[]}"#;
    assert_eq!(
        receiver
            .handle(HealthRequest {
                authorization: None,
                body: payload
            })
            .status,
        401
    );
    let response = receiver.handle(HealthRequest {
        authorization: Some("Bearer fixture-secret"),
        body: payload,
    });
    assert_eq!(response.status, 200);
    assert_eq!(response.body, r#"{"inserted":0,"duplicates":0}"#);
    assert_eq!(
        receiver
            .handle(HealthRequest {
                authorization: Some("Bearer fixture-secret"),
                body: b"bad"
            })
            .status,
        400
    );
    assert_eq!(
        receiver
            .handle(HealthRequest {
                authorization: Some("Bearer fixture-secret"),
                body: &vec![b' '; 1_048_577]
            })
            .status,
        413
    );
}

#[test]
fn receiver_authorization_matches_the_exact_bearer_value() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let receiver = HealthReceiver::new("fixture-secret", HealthImporter::new(database));
    for authorization in [
        None,
        Some(""),
        Some("fixture-secret"),
        Some("bearer fixture-secret"),
        Some("Bearer"),
        Some("Bearer  fixture-secret"),
        Some("Bearer fixture-secret "),
        Some("Bearer fixture-secret-suffix"),
        Some("prefix-Bearer fixture-secret"),
        Some("Bearer gixture-recret"),
    ] {
        let response = receiver.handle(HealthRequest {
            authorization,
            body: b"not parsed when unauthorized",
        });
        assert_eq!(response.status, 401, "{authorization:?}");
        assert_eq!(response.body, r#"{"error":"unauthorized"}"#);
    }

    assert_eq!(
        receiver
            .handle(HealthRequest {
                authorization: Some("Bearer fixture-secret"),
                body: br#"{"workouts":[],"metrics":[]}"#,
            })
            .status,
        200
    );
}
