// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! WebSocket connection management: URL construction, header injection,
//! skew-retry, and a thin send/receive facade.
//!
//! [`Ws::connect`] handles the full handshake including the rotating
//! `Sec-MS-GEC` token and retries on clock-skew-induced 403 responses.

use crate::auth::{self, random_hex, SEC_MS_GEC_VERSION, TRUSTED_CLIENT_TOKEN};
use crate::error::TtsError;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Message},
};

/// Edge Read-Aloud endpoint base path.
const ENDPOINT_BASE: &str = "speech.platform.bing.com/consumer/speech/synthesize/readaloud";

/// Chrome-extension origin the Edge browser sends (the server checks this).
const ORIGIN: &str = "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold";

/// User-Agent mimicking Edge stable; the server rejects blank or non-browser UAs.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

/// HTTP no-cache directive (used for both pragma and cache-control headers).
const NO_CACHE: &str = "no-cache";
const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";
const ACCEPT_ENCODING: &str = "gzip, deflate, br, zstd";
const HTTP_FORBIDDEN: &str = "403";
const HTTP_UNAUTHORIZED: &str = "401";

/// Maximum connect attempts before giving up.  Any transport or handshake
/// error (`TtsError::Ws`) is retried; only a 403/401 nudges the clock skew
/// between attempts.
const MAX_CONNECT_ATTEMPTS: u8 = 3;

/// Seconds added to our clock per retry when the server reports 403 (clock skew).
const SKEW_NUDGE_SECS: i64 = 60;

/// Per-attempt timeout for DNS + TCP + TLS + WebSocket upgrade.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

const HEADER_ORIGIN: &str = "origin";
const HEADER_USER_AGENT: &str = "user-agent";
const HEADER_COOKIE: &str = "cookie";
const HEADER_PRAGMA: &str = "pragma";
const HEADER_CACHE_CONTROL: &str = "cache-control";
const HEADER_ACCEPT_LANGUAGE: &str = "accept-language";
const HEADER_ACCEPT_ENCODING: &str = "accept-encoding";

/// MUID cookie byte length (random hex -> 32 hex chars).
const MUID_BYTES: usize = 16;

/// ConnectionId / X-RequestId byte length.
const CONN_ID_BYTES: usize = 16;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A connected Edge-TTS WebSocket with a thin send/receive facade.
pub(crate) struct Ws {
    inner: WsStream,
}

impl Ws {
    /// Connect with skew-retry.  A 403/401 response implies clock drift; we
    /// nudge the skew and retry up to [`MAX_CONNECT_ATTEMPTS`] times.
    pub(crate) async fn connect() -> Result<Self, TtsError> {
        let mut skew = 0i64;
        let mut last_err = String::new();

        for attempt in 0..MAX_CONNECT_ATTEMPTS {
            match Self::try_connect(skew).await {
                Ok(ws) => return Ok(ws),
                Err(TtsError::Ws(e)) => {
                    let msg = e.to_string();
                    last_err = msg.clone();
                    if msg.contains(HTTP_FORBIDDEN) || msg.contains(HTTP_UNAUTHORIZED) {
                        skew += SKEW_NUDGE_SECS;
                    }
                    log::warn!("edge-tts connect attempt {attempt} failed: {msg} (skew={skew})");
                }
                Err(e) => return Err(e),
            }
        }
        Err(TtsError::Connect(last_err))
    }

    async fn try_connect(skew: i64) -> Result<Self, TtsError> {
        let gec = auth::sec_ms_gec(skew);
        let muid = random_hex(MUID_BYTES);
        let conn_id = random_hex(CONN_ID_BYTES);

        let url = format!(
            "wss://{ENDPOINT_BASE}/edge/v1\
             ?TrustedClientToken={TRUSTED_CLIENT_TOKEN}\
             &ConnectionId={conn_id}\
             &Sec-MS-GEC={gec}\
             &Sec-MS-GEC-Version={SEC_MS_GEC_VERSION}"
        );

        let mut req = url.into_client_request()?;
        let headers = req.headers_mut();
        headers.insert(HEADER_ORIGIN, HeaderValue::from_static(ORIGIN));
        headers.insert(HEADER_USER_AGENT, HeaderValue::from_static(USER_AGENT));
        headers.insert(
            HEADER_COOKIE,
            HeaderValue::from_str(&format!("muid={muid};"))
                .map_err(|e| TtsError::Connect(format!("bad cookie header: {e}")))?,
        );
        headers.insert(HEADER_PRAGMA, HeaderValue::from_static(NO_CACHE));
        headers.insert(HEADER_CACHE_CONTROL, HeaderValue::from_static(NO_CACHE));
        headers.insert(
            HEADER_ACCEPT_LANGUAGE,
            HeaderValue::from_static(ACCEPT_LANGUAGE),
        );
        headers.insert(
            HEADER_ACCEPT_ENCODING,
            HeaderValue::from_static(ACCEPT_ENCODING),
        );

        let (inner, resp) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(req))
            .await
            .map_err(|_| {
                TtsError::Connect(format!(
                    "WebSocket connect timed out after {CONNECT_TIMEOUT:?}"
                ))
            })??;
        log::debug!("edge-tts WS connected: HTTP {}", resp.status());
        Ok(Ws { inner })
    }

    /// Send a text protocol message.
    pub(crate) async fn send_text(&mut self, msg: String) -> Result<(), TtsError> {
        self.inner.send(Message::Text(msg)).await?;
        Ok(())
    }

    /// Reply to a WebSocket Ping with a Pong (best-effort).
    pub(crate) async fn send_pong(&mut self, data: Vec<u8>) {
        // best-effort: a pong failure will surface as a recv error on the next read
        let _ = self.inner.send(Message::Pong(data)).await;
    }

    /// Receive the next message, or `Ok(None)` if the server closed the
    /// stream, or `Err(TimedOut)` if no message arrives within `timeout`.
    pub(crate) async fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Message>, TtsError> {
        match tokio::time::timeout(timeout, self.inner.next()).await {
            Ok(Some(msg)) => Ok(Some(msg?)),
            Ok(None) => Ok(None),
            Err(_) => Err(TtsError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "edge-tts receive idle timeout exceeded",
            ))),
        }
    }
}
