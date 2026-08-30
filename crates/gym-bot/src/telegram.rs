//! `teloxide` adapter kept at the transport edge.

use botkit::{SurfaceId, UpdateKey, ValidationError};
use teloxide::types::{Update, UpdateKind};
use thiserror::Error;

use crate::command::CommandInput;

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
