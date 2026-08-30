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
