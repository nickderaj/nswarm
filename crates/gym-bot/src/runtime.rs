//! Production Telegram lifecycle over transport-neutral gym services.

use std::sync::Arc;

use chrono::{DateTime, FixedOffset};
use teloxide::{
    Bot,
    payloads::{AnswerCallbackQuerySetters, GetUpdatesSetters},
    prelude::Requester,
    types::{Update, UpdateKind},
};
use thiserror::Error;

use crate::{
    batch::{BatchError, BatchService},
    clock::Clock,
    command::{CommandError, CommandInput, UpdateGate},
    service::{GymService, ServiceError, ServiceRequest},
    telegram::{
        PreferenceCallbackInput, TelegramAdapterError, adapt_preference_callback, adapt_update,
    },
};

const AGENT_UNAVAILABLE: &str =
    "Agent-dependent gym behavior is unavailable while architecture decision D23 is unresolved.";
const BATCH_LIMIT: usize = 20;
const MAX_BATCH_ENTRY_BYTES: usize = 10_000;

/// Owner-guarded, restart-safe bridge from adapter inputs to gym services.
pub struct RuntimeService {
    gate: UpdateGate,
    gym: GymService,
    batch: BatchService,
    clock: Arc<dyn Clock>,
}

impl RuntimeService {
    /// Builds the production route over the copied gym database and v1 sidecar.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] if the copied database or sidecar is invalid.
    pub fn new(
        owner_id: impl Into<String>,
        database_path: impl Into<std::path::PathBuf>,
        processed_updates_path: impl AsRef<std::path::Path>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, RuntimeError> {
        let database_path = database_path.into();
        Ok(Self {
            gate: UpdateGate::new(owner_id, &database_path, processed_updates_path)?,
            gym: GymService::new(&database_path, Arc::clone(&clock)),
            batch: BatchService::new(database_path),
            clock,
        })
    }

    /// Handles an adapted message after owner-first durable deduplication.
    ///
    /// `source_message_id` is the stable upstream message identity used only
    /// for the frozen batch-buffer uniqueness contract.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] if durable gating or domain storage fails.
    pub fn handle_message(
        &self,
        input: &CommandInput,
        conversation_id: i64,
        source_message_id: i64,
    ) -> Result<Option<String>, RuntimeError> {
        if self.gate.claim(input)?.is_some() {
            return Ok(None);
        }
        let text = input.text.trim();
        if !text.starts_with('/') && self.batch.active(conversation_id)? {
            if text.is_empty() || text.len() > MAX_BATCH_ENTRY_BYTES {
                return Ok(Some(
                    "Batch entries must contain 1..=10000 characters.".to_owned(),
                ));
            }
            let sent_at = DateTime::<FixedOffset>::parse_from_rfc3339(&self.clock.now_iso8601())?;
            self.batch
                .append(conversation_id, source_message_id, text, sent_at)?;
            let (count, _) = self.batch.status(conversation_id)?;
            return Ok((count >= BATCH_LIMIT).then(|| {
                format!("{AGENT_UNAVAILABLE} The {count}-message batch was kept for /batch retry.")
            }));
        }
        self.gym
            .handle(&ServiceRequest {
                conversation_id: conversation_id.to_string(),
                text: input.text.clone(),
            })
            .map(Some)
            .map_err(RuntimeError::from)
    }

    /// Handles an adapted Keep/Reject callback after the same durable gate.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] if durable gating or the one-shot update fails.
    pub fn handle_preference_callback(
        &self,
        input: &PreferenceCallbackInput,
    ) -> Result<Option<String>, RuntimeError> {
        let claim = CommandInput {
            actor_id: input.actor_id.clone(),
            update: input.update.clone(),
            text: "preference-review".to_owned(),
        };
        if self.gate.claim(&claim)?.is_some() {
            return Ok(None);
        }
        self.gym
            .review_preference(input.review)
            .map(Some)
            .map_err(RuntimeError::from)
    }
}

/// Polls Telegram with the new v1 token until cancelled or a request fails.
///
/// # Errors
///
/// Returns [`RuntimeError`] when Telegram or a deterministic route fails.
pub async fn run_telegram(token: String, service: Arc<RuntimeService>) -> Result<(), RuntimeError> {
    let bot = Bot::new(token);
    let mut offset = 0;
    loop {
        let updates = bot.get_updates().offset(offset).timeout(30).await?;
        for update in updates {
            let next_offset = update.id.as_offset();
            dispatch_update(&bot, &service, &update).await?;
            offset = offset.max(next_offset);
        }
    }
}

async fn dispatch_update(
    bot: &Bot,
    service: &RuntimeService,
    update: &Update,
) -> Result<(), RuntimeError> {
    if let UpdateKind::CallbackQuery(query) = &update.kind {
        match adapt_preference_callback(update) {
            Ok(Some(input)) => {
                let text = service
                    .handle_preference_callback(&input)?
                    .unwrap_or_else(|| "This review action was already handled.".to_owned());
                bot.answer_callback_query(query.id.clone())
                    .text(text)
                    .await?;
            }
            Ok(None) => {}
            Err(TelegramAdapterError::InvalidPreferenceCallback) => {
                bot.answer_callback_query(query.id.clone())
                    .text("This review action is invalid.")
                    .show_alert(true)
                    .await?;
            }
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }

    let UpdateKind::Message(message) = &update.kind else {
        return Ok(());
    };
    if message.text().is_none() {
        return Ok(());
    }
    let Some(input) = adapt_update(update)? else {
        return Ok(());
    };
    if let Some(text) =
        service.handle_message(&input, message.chat.id.0, i64::from(message.id.0))?
    {
        bot.send_message(message.chat.id, text).await?;
    }
    Ok(())
}

/// Supervised Telegram/runtime failure with secret-safe diagnostics.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Owner gate or processed-update sidecar failed.
    #[error(transparent)]
    Command(#[from] CommandError),
    /// Deterministic gym behavior failed.
    #[error(transparent)]
    Service(#[from] ServiceError),
    /// Durable batch storage failed.
    #[error(transparent)]
    Batch(#[from] BatchError),
    /// Injected clock did not return RFC-3339.
    #[error("gym runtime clock returned an invalid timestamp: {0}")]
    Time(#[from] chrono::ParseError),
    /// Telegram update adaptation failed.
    #[error(transparent)]
    TelegramAdapter(#[from] TelegramAdapterError),
    /// Telegram API request failed without exposing the token.
    #[error("Telegram request failed: {0}")]
    Telegram(#[from] teloxide::RequestError),
}
