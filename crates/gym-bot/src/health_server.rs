//! Bounded HTTP/1.1 listener for the Apple Health receiver contract.

use std::{
    collections::VecDeque,
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::receiver::{HealthReceiver, HealthRequest, HealthResponse};

const MAX_HEADER_BYTES: usize = 16_384;
const MAX_BODY_BYTES: usize = 1_048_576;
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const REQUESTS_PER_MINUTE: usize = 30;

/// Runs the production Health receiver until the task is cancelled.
///
/// # Errors
///
/// Returns [`HealthServerError`] if the listener cannot bind or accept.
pub async fn run_health_server(
    address: SocketAddr,
    receiver: Arc<HealthReceiver>,
) -> Result<(), HealthServerError> {
    let listener = TcpListener::bind(address).await?;
    run_health_listener(listener, receiver).await
}

async fn run_health_listener(
    listener: TcpListener,
    receiver: Arc<HealthReceiver>,
) -> Result<(), HealthServerError> {
    let limiter = Arc::new(RateLimiter::default());
    loop {
        let (stream, _) = listener.accept().await?;
        let receiver = Arc::clone(&receiver);
        let limiter = Arc::clone(&limiter);
        std::mem::drop(tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, &receiver, &limiter).await {
                eprintln!("gym Health connection failed: {error}");
            }
        }));
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    receiver: &HealthReceiver,
    limiter: &RateLimiter,
) -> Result<(), HealthConnectionError> {
    let response = match timeout(READ_TIMEOUT, read_request(&mut stream)).await {
        Ok(Ok(request)) => dispatch_request(receiver, limiter, &request),
        Ok(Err(error)) => error.response(),
        Err(_) => json_response(408, r#"{"error":"request timeout"}"#),
    };
    write_response(&mut stream, &response).await?;
    Ok(())
}

fn dispatch_request(
    receiver: &HealthReceiver,
    limiter: &RateLimiter,
    request: &ParsedRequest,
) -> HealthResponse {
    if request.method != "POST" || request.path != "/import/health" {
        return json_response(404, r#"{"error":"not found"}"#);
    }
    if !limiter.allow(Instant::now()) {
        return json_response(429, r#"{"error":"rate limit exceeded"}"#);
    }
    receiver.handle(HealthRequest {
        authorization: request.authorization.as_deref(),
        body: &request.body,
    })
}

async fn read_request(stream: &mut TcpStream) -> Result<ParsedRequest, RequestError> {
    let mut bytes = Vec::with_capacity(4096);
    let header_end = loop {
        if bytes.len() >= MAX_HEADER_BYTES {
            return Err(RequestError::HeadersTooLarge);
        }
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(RequestError::Incomplete);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = find_header_end(&bytes) {
            break position;
        }
    };
    if header_end > MAX_HEADER_BYTES {
        return Err(RequestError::HeadersTooLarge);
    }
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| RequestError::Malformed)?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or(RequestError::Malformed)?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or(RequestError::Malformed)?
        .to_owned();
    let path = request_parts
        .next()
        .ok_or(RequestError::Malformed)?
        .to_owned();
    let version = request_parts.next().ok_or(RequestError::Malformed)?;
    if request_parts.next().is_some() || version != "HTTP/1.1" {
        return Err(RequestError::Malformed);
    }

    let mut authorization = None;
    let mut content_length = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(RequestError::Malformed)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            if authorization.replace(value.to_owned()).is_some() {
                return Err(RequestError::Malformed);
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length
                .replace(
                    value
                        .parse::<usize>()
                        .map_err(|_| RequestError::Malformed)?,
                )
                .is_some()
            {
                return Err(RequestError::Malformed);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(RequestError::Malformed);
        }
    }
    let content_length = content_length.ok_or(RequestError::LengthRequired)?;
    if content_length > MAX_BODY_BYTES {
        return Err(RequestError::BodyTooLarge);
    }

    let body_start = header_end + 4;
    let already_read = bytes.len().saturating_sub(body_start);
    if already_read > content_length {
        return Err(RequestError::Malformed);
    }
    while bytes.len() - body_start < content_length {
        let remaining = content_length - (bytes.len() - body_start);
        let mut chunk = vec![0_u8; remaining.min(8192)];
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(RequestError::Incomplete);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(ParsedRequest {
        method,
        path,
        authorization,
        body: bytes[body_start..].to_vec(),
    })
}

#[doc(hidden)]
#[must_use]
pub fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_response(
    stream: &mut TcpStream,
    response: &HealthResponse,
) -> Result<(), io::Error> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(response.body.as_bytes()).await?;
    stream.shutdown().await
}

fn json_response(status: u16, body: &str) -> HealthResponse {
    HealthResponse {
        status,
        body: body.to_owned(),
    }
}

struct ParsedRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct RateLimiter {
    requests: Mutex<VecDeque<Instant>>,
}

impl RateLimiter {
    #[doc(hidden)]
    #[must_use]
    pub fn allow(&self, now: Instant) -> bool {
        let Ok(mut requests) = self.requests.lock() else {
            return false;
        };
        let cutoff = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        while requests.front().is_some_and(|request| *request <= cutoff) {
            requests.pop_front();
        }
        if requests.len() >= REQUESTS_PER_MINUTE {
            return false;
        }
        requests.push_back(now);
        true
    }
}

#[derive(Debug, Error)]
enum RequestError {
    #[error("Health request I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Health request is malformed")]
    Malformed,
    #[error("Health request headers are too large")]
    HeadersTooLarge,
    #[error("Health request body is incomplete")]
    Incomplete,
    #[error("Health request needs Content-Length")]
    LengthRequired,
    #[error("Health request body is too large")]
    BodyTooLarge,
}

impl RequestError {
    fn response(&self) -> HealthResponse {
        match self {
            Self::LengthRequired => json_response(411, r#"{"error":"content length required"}"#),
            Self::BodyTooLarge => json_response(413, r#"{"error":"payload too large"}"#),
            Self::Io(_) | Self::Malformed | Self::HeadersTooLarge | Self::Incomplete => {
                json_response(400, r#"{"error":"invalid request"}"#)
            }
        }
    }
}

#[derive(Debug, Error)]
enum HealthConnectionError {
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Production Health listener failure.
#[derive(Debug, Error)]
pub enum HealthServerError {
    /// Listener bind or accept failed.
    #[error("Health listener failed: {0}")]
    Io(#[from] io::Error),
}
