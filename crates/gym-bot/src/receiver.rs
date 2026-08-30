//! Network-neutral Apple Health HTTP receiver contract.

use crate::health::{HealthError, HealthImporter};

/// Minimal request accepted at the HTTP adapter edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthRequest<'a> {
    /// Authorization header value.
    pub authorization: Option<&'a str>,
    /// Raw request body.
    pub body: &'a [u8],
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
        let expected = format!("Bearer {}", self.bearer_token);
        if !constant_time_equal(
            request.authorization.unwrap_or_default().as_bytes(),
            expected.as_bytes(),
        ) {
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
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn response(status: u16, body: &str) -> HealthResponse {
    HealthResponse {
        status,
        body: body.to_owned(),
    }
}
