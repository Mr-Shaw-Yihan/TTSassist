// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! HTTPS fetch of the Edge TTS voice catalogue.

use crate::auth::{self, SEC_MS_GEC_VERSION, TRUSTED_CLIENT_TOKEN};
use crate::error::TtsError;
use crate::voice_types::VoiceInfo;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

const VOICE_HOST: &str = "speech.platform.bing.com";
const VOICE_PATH: &str = "/consumer/speech/synthesize/readaloud/voices/list";
const HTTPS_PORT: u16 = 443;

const FETCH_USER_AGENT: &str = "Mozilla/5.0";
const FETCH_HOST_HEADER: &str = "Host";
const FETCH_CONNECTION_HEADER: &str = "Connection";
const FETCH_CLOSE_VALUE: &str = "close";
const FETCH_USER_AGENT_HEADER: &str = "User-Agent";

const FETCH_BUFFER_CAPACITY: usize = 64 * 1024;

/// Per-read chunk size. Kept small so the fetch future stays compact while the
/// size cap below still aborts a runaway stream after a bounded number of reads.
const FETCH_READ_CHUNK: usize = 8 * 1024;

/// Upper bound on an accepted voice-list response. The real catalogue is well
/// under 1 MB; anything larger signals a misbehaving server or an unterminated
/// stream and is rejected rather than read unbounded into memory.
const MAX_VOICE_LIST_BYTES: usize = 4 * 1024 * 1024;

/// Wall-clock cap on one connect + download. A server that holds the socket
/// open otherwise stalls the retry loop and leaks the fetch thread and socket.
const FETCH_OP_TIMEOUT: Duration = Duration::from_secs(20);

/// How many times to try the catalogue fetch before giving up. The list call
/// often runs the instant WiFi connects, when the resolver is not ready yet
/// (`EAI_AGAIN`); a few retries spaced a few seconds apart let DNS catch up.
const FETCH_MAX_ATTEMPTS: u32 = 6;
const FETCH_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Fetch the complete Edge-TTS voice catalogue.
///
/// Requires [`crate::init_tls()`] to have been called first (installs the
/// `ring` crypto provider).  Returns `Err` on any network or parse failure;
/// callers should fall back to a hardcoded list on error.
pub async fn list_voices() -> Result<Vec<VoiceInfo>, TtsError> {
    let gec = auth::sec_ms_gec(0);
    let path = format!(
        "{VOICE_PATH}?TrustedClientToken={TRUSTED_CLIENT_TOKEN}\
         &Sec-MS-GEC={gec}\
         &Sec-MS-GEC-Version={SEC_MS_GEC_VERSION}"
    );

    let body = https_get(VOICE_HOST, &path).await?;
    serde_json::from_str(&body).map_err(|e| TtsError::Parse(format!("voice list JSON parse: {e}")))
}

/// Spawn a blocking thread that fetches the voice list and returns it
/// via a channel. The caller receives a `Receiver` that yields the
/// voices once the fetch completes, or drops on failure.
pub fn spawn_voice_fetch() -> std::sync::mpsc::Receiver<Vec<VoiceInfo>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::warn!("voice fetch: runtime creation failed: {e}");
                return;
            }
        };
        for attempt in 1..=FETCH_MAX_ATTEMPTS {
            match rt.block_on(list_voices()) {
                Ok(voices) => {
                    // best-effort: a dropped receiver means the caller already
                    // fell back to the static table; nothing left to deliver.
                    let _ = tx.send(voices);
                    return;
                }
                Err(e) => {
                    log::warn!(
                        "voice list fetch attempt {attempt}/{} failed: {e}",
                        FETCH_MAX_ATTEMPTS
                    );
                    if attempt < FETCH_MAX_ATTEMPTS {
                        std::thread::sleep(FETCH_RETRY_DELAY);
                    }
                }
            }
        }
        log::warn!(
            "voice list fetch gave up after {} attempts",
            FETCH_MAX_ATTEMPTS
        );
    });
    rx
}

async fn https_get(host: &str, path: &str) -> Result<String, TtsError> {
    let connector = tls_connector()?;

    let dns_name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|e| TtsError::VoiceFetch(format!("invalid DNS name: {e}")))?;

    let raw = tokio::time::timeout(FETCH_OP_TIMEOUT, async move {
        let tcp = TcpStream::connect((host, HTTPS_PORT))
            .await
            .map_err(TtsError::Io)?;
        let mut tls = connector.connect(dns_name, tcp).await?;

        let request = format!(
            "GET {path} HTTP/1.1\r\n\
             {FETCH_HOST_HEADER}: {host}\r\n\
             {FETCH_CONNECTION_HEADER}: {FETCH_CLOSE_VALUE}\r\n\
             {FETCH_USER_AGENT_HEADER}: {FETCH_USER_AGENT}\r\n\
             \r\n"
        );
        tls.write_all(request.as_bytes()).await?;

        let mut raw = Vec::with_capacity(FETCH_BUFFER_CAPACITY);
        let mut chunk = [0u8; FETCH_READ_CHUNK];
        loop {
            let n = tls.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
            if raw.len() > MAX_VOICE_LIST_BYTES {
                return Err(TtsError::VoiceFetch(format!(
                    "voice list response too large: {} bytes (cap {MAX_VOICE_LIST_BYTES})",
                    raw.len()
                )));
            }
        }
        Ok(raw)
    })
    .await
    .map_err(|_| {
        TtsError::VoiceFetch(format!(
            "voice list fetch timed out after {FETCH_OP_TIMEOUT:?}"
        ))
    })??;

    let response = String::from_utf8_lossy(&raw);
    let body_start = response
        .find(crate::protocol::HEADER_BODY_SEPARATOR)
        .map(|i| i + crate::protocol::HEADER_BODY_SEPARATOR.len())
        .ok_or_else(|| TtsError::VoiceFetch("malformed HTTP response".into()))?;

    Ok(response.get(body_start..).unwrap_or("").to_string())
}

fn tls_connector() -> Result<TlsConnector, TtsError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(TlsConnector::from(std::sync::Arc::new(config)))
}
