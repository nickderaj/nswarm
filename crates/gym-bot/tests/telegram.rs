//! Type-only teloxide adapter tests with recorded JSON shapes.

use gym_bot::service::{PreferenceReviewDecision, PreferenceReviewRequest};
use gym_bot::telegram::{
    DispatchError, DispatchResult, TelegramAdapterError, TelegramReply,
    decode_preference_callback_json, decode_update_json, dispatch_recorded,
};

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

const PREFERENCE_CALLBACK: &str = r#"{
  "update_id": 54,
  "callback_query": {
    "id": "callback-1",
    "from": {"id": 700, "is_bot": false, "first_name": "Fixture"},
    "chat_instance": "fixture-chat",
    "message": {
      "message_id": 8,
      "date": 0,
      "chat": {"id": 700, "type": "private", "first_name": "Fixture"},
      "text": "Coaching preference proposal"
    },
    "data": "gym-preference:41:accept"
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
fn recorded_update_dispatches_through_the_neutral_core() {
    let result = dispatch_recorded(VALID, |input| {
        assert_eq!(input.actor_id, "700");
        Ok(Some("recorded reply".to_owned()))
    })
    .expect("dispatch fixture");
    assert_eq!(
        result,
        DispatchResult::Reply(TelegramReply {
            conversation_id: "700".to_owned(),
            text: "recorded reply".to_owned()
        })
    );
}

#[test]
fn dispatch_supports_ignored_and_core_error_results() {
    assert!(matches!(
        dispatch_recorded("not-json", |_| Ok(None)),
        Err(DispatchError::Adapter(TelegramAdapterError::Json(_)))
    ));
    assert_eq!(
        dispatch_recorded(r#"{"update_id":53,"future_update":{"safe":true}}"#, |_| Ok(
            Some("unused".to_owned())
        ))
        .expect("ignored"),
        DispatchResult::Ignored
    );
    assert!(
        matches!(dispatch_recorded(VALID, |_| Err("unavailable".to_owned())), Err(DispatchError::Core(value)) if value == "unavailable")
    );
    assert_eq!(
        dispatch_recorded(VALID, |_| Ok(None)).expect("no reply"),
        DispatchResult::Ignored
    );
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

#[test]
fn preference_callback_adapts_to_stable_neutral_keep_and_reject_inputs() {
    let keep = decode_preference_callback_json(PREFERENCE_CALLBACK)
        .expect("valid callback")
        .expect("gym preference callback");
    assert_eq!(keep.actor_id, "700");
    assert_eq!(keep.update.surface.as_str(), "telegram");
    assert_eq!(keep.update.external_id, "54");
    assert_eq!(
        keep.review,
        PreferenceReviewRequest {
            preference_id: 41,
            decision: PreferenceReviewDecision::Keep,
        }
    );

    let reject =
        decode_preference_callback_json(&PREFERENCE_CALLBACK.replace("41:accept", "42:reject"))
            .expect("valid reject")
            .expect("gym preference callback");
    assert_eq!(
        reject.review,
        PreferenceReviewRequest {
            preference_id: 42,
            decision: PreferenceReviewDecision::Reject,
        }
    );
}

#[test]
fn preference_callback_ignores_other_updates_and_rejects_malformed_own_namespace() {
    assert!(
        decode_preference_callback_json(VALID)
            .expect("message is unrelated")
            .is_none()
    );
    assert!(
        decode_preference_callback_json(
            &PREFERENCE_CALLBACK.replace("gym-preference:41:accept", "other:41:accept")
        )
        .expect("other callback namespace")
        .is_none()
    );
    for data in [
        "gym-preference:not-a-number:accept",
        "gym-preference:41:maybe",
        "gym-preference:41:accept:extra",
        "gym-preference:",
    ] {
        assert!(matches!(
            decode_preference_callback_json(
                &PREFERENCE_CALLBACK.replace("gym-preference:41:accept", data)
            ),
            Err(TelegramAdapterError::MissingText)
        ));
    }

    let mut without_data: serde_json::Value =
        serde_json::from_str(PREFERENCE_CALLBACK).expect("callback JSON");
    without_data["callback_query"]
        .as_object_mut()
        .expect("callback object")
        .remove("data");
    assert!(
        decode_preference_callback_json(&without_data.to_string())
            .expect("callback without data is unrelated")
            .is_none()
    );
}
