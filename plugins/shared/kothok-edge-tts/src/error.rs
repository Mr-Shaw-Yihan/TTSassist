// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Crate-level error type for Edge-TTS synthesis failures.

use thiserror::Error;

/// All ways a synthesis request can fail.
///
/// Callers typically propagate this with `?` or match on a variant to
/// decide whether to retry (e.g. [`TtsError::Connect`] after a network blip).
#[derive(Debug, Error)]
pub enum TtsError {
    /// The WebSocket handshake or DRM-token auth failed after all retry
    /// attempts. The inner string is the last underlying error message.
    #[error("ws connect failed after retries: {0}")]
    Connect(String),

    /// The voice-catalogue HTTPS fetch failed (DNS, transport, HTTP framing).
    /// The inner string is the underlying error message.
    #[error("voice fetch failed: {0}")]
    VoiceFetch(String),

    /// A voice-list JSON body could not be deserialized. The inner string is
    /// the underlying serde error message.
    #[error("parse failed: {0}")]
    Parse(String),

    /// Transport-level WebSocket error (handshake, frame decode, TLS).
    ///
    /// The `tungstenite::Error` is boxed to keep `TtsError` small: it is the
    /// largest variant and threads through every `Result<_, TtsError>` on the
    /// stack. Boxing it allocates only on the rare error path.
    #[error("ws: {0}")]
    Ws(#[source] Box<tokio_tungstenite::tungstenite::Error>),

    /// I/O error on the underlying TCP stream, including receive idle timeout.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// The turn completed (or the stream closed) without any audio frames.
    #[error("no audio received")]
    NoAudio,
}

impl From<tokio_tungstenite::tungstenite::Error> for TtsError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        TtsError::Ws(Box::new(e))
    }
}
