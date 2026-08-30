//! Durable batch state and restart-safe due claims.

use std::path::PathBuf;

use chrono::{DateTime, Duration, FixedOffset};
use rusqlite::{OptionalExtension, params};
use thiserror::Error;

use crate::database::{DatabaseError, open_existing};

/// One buffered transport-neutral message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchEntry {
    /// Stable row identity used for snapshot-only discard.
    pub id: i64,
    /// Upstream message identity.
    pub message_id: i64,
    /// Plain message text.
    pub text: String,
    /// RFC-3339 source timestamp.
    pub sent_at: String,
}

/// Durable batch repository and scheduling service.
pub struct BatchService {
    database_path: PathBuf,
}

impl BatchService {
    /// Creates a service against the frozen v0 batch tables.
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
    }

    /// Reports whether a conversation currently has an open batch.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when storage is unavailable.
    pub fn active(&self, chat_id: i64) -> Result<bool, BatchError> {
        let connection = open_existing(&self.database_path)?;
        Ok(connection
            .query_row(
                "SELECT 1 FROM batch_state WHERE chat_id=?1",
                [chat_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Returns the buffered message count and earliest source timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when storage is unavailable.
    pub fn status(&self, chat_id: i64) -> Result<(usize, Option<String>), BatchError> {
        let connection = open_existing(&self.database_path)?;
        let (count, earliest): (i64, Option<String>) = connection.query_row(
            "SELECT count(*), min(sent_at) FROM batch_buffer WHERE chat_id=?1",
            [chat_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((
            usize::try_from(count).map_err(|_| BatchError::CountOutOfRange)?,
            earliest,
        ))
    }

    /// Opens or refreshes a batch with its persisted 12-hour deadline.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when storage is unavailable.
    pub fn open(&self, chat_id: i64, now: DateTime<FixedOffset>) -> Result<(), BatchError> {
        let connection = open_existing(&self.database_path)?;
        connection.execute(
            "INSERT INTO batch_state (chat_id,opened_at,auto_flush_at) VALUES (?1,?2,?3) \
             ON CONFLICT(chat_id) DO UPDATE SET opened_at=excluded.opened_at,auto_flush_at=excluded.auto_flush_at",
            params![chat_id, now.to_rfc3339(), (now + Duration::hours(12)).to_rfc3339()],
        )?;
        Ok(())
    }

    /// Adds one source message exactly once while the batch is active.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] for invalid text or unavailable storage.
    pub fn append(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        sent_at: DateTime<FixedOffset>,
    ) -> Result<bool, BatchError> {
        if text.trim().is_empty() || text.len() > 10_000 {
            return Err(BatchError::InvalidEntry);
        }
        let connection = open_existing(&self.database_path)?;
        let active = connection
            .query_row(
                "SELECT 1 FROM batch_state WHERE chat_id=?1",
                [chat_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !active {
            return Ok(false);
        }
        Ok(connection.execute(
            "INSERT INTO batch_buffer (chat_id,message_id,text,sent_at) VALUES (?1,?2,?3,?4) \
             ON CONFLICT(chat_id,message_id) DO NOTHING",
            params![chat_id, message_id, text, sent_at.to_rfc3339()],
        )? == 1)
    }

    /// Returns all due chat identities in stable order.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when storage is unavailable.
    pub fn due(&self, now: DateTime<FixedOffset>) -> Result<Vec<i64>, BatchError> {
        let connection = open_existing(&self.database_path)?;
        let mut statement = connection
            .prepare("SELECT chat_id FROM batch_state WHERE auto_flush_at<=?1 ORDER BY chat_id")?;
        Ok(statement
            .query_map([now.to_rfc3339()], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Returns a stable flush snapshot without deleting it.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when storage is unavailable.
    pub fn snapshot(&self, chat_id: i64) -> Result<Vec<BatchEntry>, BatchError> {
        let connection = open_existing(&self.database_path)?;
        let mut statement = connection.prepare("SELECT id,message_id,text,sent_at FROM batch_buffer WHERE chat_id=?1 ORDER BY sent_at,message_id")?;
        Ok(statement
            .query_map([chat_id], |row| {
                Ok(BatchEntry {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    text: row.get(2)?,
                    sent_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Discards only a successfully processed snapshot; later messages survive.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when the transactional update fails.
    pub fn complete(&self, chat_id: i64, snapshot: &[BatchEntry]) -> Result<(), BatchError> {
        if snapshot.is_empty() {
            return Ok(());
        }
        let mut connection = open_existing(&self.database_path)?;
        let transaction = connection.transaction()?;
        for entry in snapshot {
            transaction.execute(
                "DELETE FROM batch_buffer WHERE chat_id=?1 AND id=?2",
                params![chat_id, entry.id],
            )?;
        }
        let remaining = transaction
            .query_row(
                "SELECT 1 FROM batch_buffer WHERE chat_id=?1 LIMIT 1",
                [chat_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !remaining {
            transaction.execute("DELETE FROM batch_state WHERE chat_id=?1", [chat_id])?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Cancels a batch and returns the number of discarded rows.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError`] when the transactional update fails.
    pub fn cancel(&self, chat_id: i64) -> Result<usize, BatchError> {
        let mut connection = open_existing(&self.database_path)?;
        let transaction = connection.transaction()?;
        let count: i64 = transaction.query_row(
            "SELECT count(*) FROM batch_buffer WHERE chat_id=?1",
            [chat_id],
            |row| row.get(0),
        )?;
        transaction.execute("DELETE FROM batch_buffer WHERE chat_id=?1", [chat_id])?;
        transaction.execute("DELETE FROM batch_state WHERE chat_id=?1", [chat_id])?;
        transaction.commit()?;
        usize::try_from(count).map_err(|_| BatchError::CountOutOfRange)
    }
}

/// Durable batch failure.
#[derive(Debug, Error)]
pub enum BatchError {
    /// Existing storage is unavailable or incompatible.
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// Fixed batch SQL failed.
    #[error("batch storage failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Message text violated the bounded contract.
    #[error("batch entry must contain 1..=10000 characters")]
    InvalidEntry,
    /// `SQLite` returned an impossible negative count.
    #[error("batch count is outside the supported range")]
    CountOutOfRange,
}
