//! Network-neutral Apple Health HTTP receiver contract.

use std::fmt::{Debug, Formatter};

use crate::health::{HealthError, HealthImporter};

/// Minimal request accepted at the HTTP adapter edge.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct HealthRequest<'a> {
    /// Authorization header value.
    pub authorization: Option<&'a str>,
    /// Raw request body.
    pub body: &'a [u8],
}

impl Debug for HealthRequest<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HealthRequest")
            .field("authorization", &self.authorization.map(|_| "[REDACTED]"))
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Minimal response emitted by the HTTP adapter edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthResponse {
    /// HTTP-compatible status.
    pub status: u16,
    /// JSON-compatible response body.
    pub body: String,
}

/// Constant-shape receiver handler with no network dependency.
pub struct HealthReceiver {
    bearer_token: String,
    importer: HealthImporter,
}

impl HealthReceiver {
    /// Creates a receiver from a secret token and deterministic importer.
    #[must_use]
    pub fn new(bearer_token: impl Into<String>, importer: HealthImporter) -> Self {
        Self {
            bearer_token: bearer_token.into(),
            importer,
        }
    }

    /// Handles one request without exposing diagnostics or credentials.
    #[must_use]
    pub fn handle(&self, request: HealthRequest<'_>) -> HealthResponse {
        let supplied_token = request
            .authorization
            .and_then(|authorization| authorization.strip_prefix("Bearer "));
        if supplied_token.is_none_or(|token| {
            !constant_time_equal(token.as_bytes(), self.bearer_token.as_bytes())
        }) {
            return response(401, r#"{"error":"unauthorized"}"#);
        }
        match self.importer.import_json(request.body) {
            Ok(result) => response(
                200,
                &format!(
                    r#"{{"inserted":{},"duplicates":{}}}"#,
                    result.inserted, result.duplicates
                ),
            ),
            Err(HealthError::PayloadTooLarge) => response(413, r#"{"error":"payload too large"}"#),
            Err(_) => response(400, r#"{"error":"invalid payload"}"#),
        }
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let length_difference = left.len() ^ right.len();
    left.iter()
        .enumerate()
        .fold(length_difference, |difference, (index, left)| {
            difference | usize::from(*left ^ right.get(index).copied().unwrap_or_default())
        })
        == 0
}

fn response(status: u16, body: &str) -> HealthResponse {
    HealthResponse {
        status,
        body: body.to_owned(),
    }
}
