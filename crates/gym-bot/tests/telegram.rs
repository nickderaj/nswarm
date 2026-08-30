//! Type-only teloxide adapter tests with recorded JSON shapes.

use gym_bot::telegram::{TelegramAdapterError, decode_update_json};

const VALID: &str = r#"{
  "update_id": 52,
  "message": {
    "message_id": 7,
    "date": 0,
    "chat": {"id": 700, "type": "private", "first_name": "Fixture"},
    "from": {"id": 700, "is_bot": false, "first_name": "Fixture"},
    "text": "/weight 82.5"
  }
}"#;

#[test]
fn teloxide_shape_converts_only_at_adapter_edge() {
    let input = decode_update_json(VALID)
        .expect("valid teloxide JSON")
        .expect("message command");
    assert_eq!(input.actor_id, "700");
    assert_eq!(input.update.surface.as_str(), "telegram");
    assert_eq!(input.update.external_id, "52");
    assert_eq!(input.text, "/weight 82.5");
}

#[test]
fn malformed_telegram_shaped_input_fails_without_network() {
    assert!(matches!(
        decode_update_json("{not-json"),
        Err(TelegramAdapterError::Json(_))
    ));
    let missing_actor = VALID.replace(
        r#""from": {"id": 700, "is_bot": false, "first_name": "Fixture"},"#,
        "",
    );
    assert!(matches!(
        decode_update_json(&missing_actor),
        Err(TelegramAdapterError::MissingActor)
    ));
    let mut missing_text: serde_json::Value =
        serde_json::from_str(VALID).expect("recorded Telegram shape");
    missing_text["message"]
        .as_object_mut()
        .expect("message object")
        .remove("text");
    assert!(matches!(
        decode_update_json(&missing_text.to_string()),
        Err(TelegramAdapterError::MissingText)
    ));
}

#[test]
fn non_message_update_is_ignored() {
    let future_shape = r#"{"update_id":53,"future_update":{"safe":true}}"#;
    assert!(
        decode_update_json(future_shape)
            .expect("teloxide preserves unknown update")
            .is_none()
    );
}
