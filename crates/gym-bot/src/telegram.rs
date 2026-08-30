//! `teloxide` adapter kept at the transport edge.

use botkit::{SurfaceId, UpdateKey, ValidationError};
use teloxide::types::{Update, UpdateKind};
use thiserror::Error;

use crate::command::CommandInput;

/// One transport-neutral outbound Telegram reply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelegramReply {
    /// Destination chat identity.
    pub conversation_id: String,
    /// Plain response text.
    pub text: String,
}

/// Deterministic adapter result for one recorded update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    /// Update was not a text message or produced no response.
    Ignored,
    /// Core produced an outbound reply.
    Reply(TelegramReply),
}

/// Handles one recorded Telegram update through a transport-neutral callback.
///
/// # Errors
///
/// Returns [`TelegramAdapterError`] when decoding fails or the callback rejects
/// the neutral input.
pub fn dispatch_recorded<F>(json: &str, mut handle: F) -> Result<DispatchResult, DispatchError>
where
    F: FnMut(&CommandInput) -> Result<Option<String>, String>,
{
    let update: Update = serde_json::from_str(json).map_err(TelegramAdapterError::Json)?;
    let destination = match &update.kind {
        UpdateKind::Message(message) => message.chat.id.to_string(),
        _ => return Ok(DispatchResult::Ignored),
    };
    let Some(input) = adapt_update(&update)? else {
        return Ok(DispatchResult::Ignored);
    };
    Ok(handle(&input)
        .map_err(DispatchError::Core)?
        .map_or(DispatchResult::Ignored, |text| {
            DispatchResult::Reply(TelegramReply {
                conversation_id: destination,
                text,
            })
        }))
}

/// Decodes Telegram JSON and converts command messages to neutral input.
///
/// # Errors
///
/// Returns [`TelegramAdapterError`] for malformed JSON or a message missing
/// its actor or text. Non-message Telegram updates return `Ok(None)`.
pub fn decode_update_json(json: &str) -> Result<Option<CommandInput>, TelegramAdapterError> {
    let update: Update = serde_json::from_str(json)?;
    adapt_update(&update)
}

/// Converts a `teloxide` update to the transport-neutral command shape.
///
/// # Errors
///
/// Returns [`TelegramAdapterError`] when a message has no actor or no text.
/// Non-message Telegram updates return `Ok(None)`.
pub fn adapt_update(update: &Update) -> Result<Option<CommandInput>, TelegramAdapterError> {
    let UpdateKind::Message(message) = &update.kind else {
        return Ok(None);
    };
    let actor_id = message
        .from
        .as_ref()
        .ok_or(TelegramAdapterError::MissingActor)?
        .id
        .0
        .to_string();
    let text = message
        .text()
        .ok_or(TelegramAdapterError::MissingText)?
        .to_owned();
    let surface = SurfaceId::new("telegram")?;
    let key = UpdateKey::new(surface, update.id.0.to_string())?;
    Ok(Some(CommandInput {
        actor_id,
        update: key,
        text,
    }))
}

/// Recorded-dispatch failure kept separate from the stable Step 2 adapter API.
#[derive(Debug, Error)]
pub enum DispatchError {
    /// Telegram decoding or adaptation failed.
    #[error(transparent)]
    Adapter(#[from] TelegramAdapterError),
    /// The transport-neutral callback failed safely.
    #[error("gym command failed: {0}")]
    Core(String),
}

/// Telegram edge decoding and adaptation failures.
#[derive(Debug, Error)]
pub enum TelegramAdapterError {
    /// The supplied shape is not a valid `teloxide` update.
    #[error("malformed Telegram update: {0}")]
    Json(#[from] serde_json::Error),
    /// A command message has no identifiable actor.
    #[error("Telegram message has no actor")]
    MissingActor,
    /// A command message contains media or another non-text payload.
    #[error("Telegram message has no text")]
    MissingText,
    /// The neutral update identity failed validation.
    #[error(transparent)]
    Identity(#[from] ValidationError),
}
