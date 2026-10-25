// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! `EdgeTts` - the concrete Edge Read-Aloud TTS engine.
//!
//! Implements [`Engine`] by opening a fresh WebSocket per synthesis request,
//! sending the `speech.config` and `ssml` messages, then collecting streamed
//! [`TtsEvent`]s until `turn.end`.

use crate::auth::{current_date_string, random_hex};
use crate::connection::Ws;
use crate::error::TtsError;
use crate::event::TtsEvent;
use crate::protocol;
use crate::protocol::LOG_BODY_MAX_CHARS;
use crate::ssml;
use crate::tts::Engine;
use std::future::Future;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const OUTPUT_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";
const RECEIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const REQ_ID_BYTES: usize = 16;

const PATH_TURN_END: &str = "turn.end";
const PATH_AUDIO_METADATA: &str = "audio.metadata";
const PATH_SPEECH_CONFIG: &str = "speech.config";
const PATH_SSML: &str = "ssml";

const HEADER_PATH: &str = "Path";

const MIME_JSON: &str = "application/json; charset=utf-8";
const MIME_SSML: &str = "application/ssml+xml";

const CFG_KEY_CONTEXT: &str = "context";
const CFG_KEY_SYNTHESIS: &str = "synthesis";
const CFG_KEY_AUDIO: &str = "audio";
const CFG_KEY_METADATA_OPTIONS: &str = "metadataoptions";
const CFG_KEY_SENTENCE_BOUNDARY_ENABLED: &str = "sentenceBoundaryEnabled";
const CFG_KEY_WORD_BOUNDARY_ENABLED: &str = "wordBoundaryEnabled";
const CFG_KEY_OUTPUT_FORMAT: &str = "outputFormat";
const CFG_VAL_FALSE: &str = "false";
const CFG_VAL_TRUE: &str = "true";

struct SynthRequest<'a> {
    text: &'a str,
    voice: &'a str,
    rate: &'a str,
    lang: &'a str,
}

/// A stateless Edge-TTS engine.
///
/// Construct `EdgeTts` (a unit struct) and call [`Engine::synthesize`].
/// Each call opens its own WebSocket; there is no connection reuse.
///
/// ```no_run
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// use kothok_edge_tts::Engine;
/// kothok_edge_tts::init_tls();
/// let events = kothok_edge_tts::EdgeTts
///     .synthesize("Hello world.", "en-US-EmmaMultilingualNeural", "+0%", "en-US")
///     .await?;
/// # Ok(()) }
/// ```
pub struct EdgeTts;

impl Engine for EdgeTts {
    fn synthesize(
        &self,
        text: &str,
        voice: &str,
        rate: &str,
        lang: &str,
    ) -> impl Future<Output = Result<Vec<TtsEvent>, TtsError>> + Send {
        let text = text.to_string();
        let voice = voice.to_string();
        let rate = rate.to_string();
        let lang = lang.to_string();
        async move {
            let req = SynthRequest {
                text: &text,
                voice: &voice,
                rate: &rate,
                lang: &lang,
            };
            Self::run_synthesis(&req).await
        }
    }
}

impl EdgeTts {
    /// Full synthesis pipeline: connect -> configure -> request -> receive.
    async fn run_synthesis(req: &SynthRequest<'_>) -> Result<Vec<TtsEvent>, TtsError> {
        let mut ws = Ws::connect().await?;
        Self::send_config(&mut ws).await?;
        Self::send_ssml(&mut ws, req).await?;
        Self::receive_events(&mut ws).await
    }

    async fn send_config(ws: &mut Ws) -> Result<(), TtsError> {
        let timestamp = current_date_string();
        let config = format!(
            "X-Timestamp:{timestamp}\r\n\
             Content-Type:{MIME_JSON}\r\n\
             Path:{PATH_SPEECH_CONFIG}\r\n\r\n\
             {{\"{CFG_KEY_CONTEXT}\":{{\"{CFG_KEY_SYNTHESIS}\":{{\"{CFG_KEY_AUDIO}\":{{\
             \"{CFG_KEY_METADATA_OPTIONS}\":{{\
             \"{CFG_KEY_SENTENCE_BOUNDARY_ENABLED}\":\"{CFG_VAL_FALSE}\",\
             \"{CFG_KEY_WORD_BOUNDARY_ENABLED}\":\"{CFG_VAL_TRUE}\"}},\
             \"{CFG_KEY_OUTPUT_FORMAT}\":\"{OUTPUT_FORMAT}\"}}}}}}}}\r\n"
        );
        ws.send_text(config).await
    }

    async fn send_ssml(ws: &mut Ws, req: &SynthRequest<'_>) -> Result<(), TtsError> {
        let request_id = random_hex(REQ_ID_BYTES);
        let timestamp = current_date_string();
        let ssml = ssml::build_ssml(req.text, req.voice, req.rate, req.lang);

        let message = format!(
            "X-RequestId:{request_id}\r\n\
             Content-Type:{MIME_SSML}\r\n\
             X-Timestamp:{timestamp}Z\r\n\
             Path:{PATH_SSML}\r\n\r\n{ssml}"
        );
        ws.send_text(message).await
    }

    /// Receive messages until `turn.end` or stream close, collecting events.
    /// Returns [`TtsError::NoAudio`] if the turn ends without any audio frames.
    async fn receive_events(ws: &mut Ws) -> Result<Vec<TtsEvent>, TtsError> {
        let mut events = Vec::new();
        let mut got_audio = false;

        while let Some(msg) = ws.recv_timeout(RECEIVE_IDLE_TIMEOUT).await? {
            match msg {
                Message::Binary(bin) => {
                    log::debug!("edge-tts BIN len={}", bin.len());
                    if let Some(audio) = protocol::parse_binary_audio(&bin) {
                        got_audio = true;
                        events.push(TtsEvent::Audio(audio));
                    }
                }
                Message::Text(text) => {
                    if Self::handle_text_message(&text, &mut events) {
                        break;
                    }
                }
                Message::Close(_) => break,
                Message::Ping(data) => ws.send_pong(data).await,
                ref other => log::debug!("edge-tts OTHER {other:?}"),
            }
        }

        if !got_audio {
            return Err(TtsError::NoAudio);
        }
        Ok(events)
    }

    /// Process one text message.  Returns `true` if the caller should stop
    /// receiving (i.e. `turn.end` was received).
    fn handle_text_message(text: &str, events: &mut Vec<TtsEvent>) -> bool {
        let (headers, body) = protocol::split_msg(text);
        let path = headers.get(HEADER_PATH).map(String::as_str).unwrap_or("");

        log::debug!(
            "edge-tts TXT path={path} body[..{LOG_BODY_MAX_CHARS}]={}",
            body.chars().take(LOG_BODY_MAX_CHARS).collect::<String>()
        );

        match path {
            PATH_TURN_END => {
                events.push(TtsEvent::TurnEnd);
                true
            }
            PATH_AUDIO_METADATA => {
                events.extend(protocol::parse_word_boundaries(body));
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_turn_end_stops_and_pushes_event() {
        let mut events = Vec::new();
        let msg = "Path:turn.end\r\n\r\n";
        let stop = EdgeTts::handle_text_message(msg, &mut events);
        assert!(stop);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], TtsEvent::TurnEnd));
    }

    #[test]
    fn handle_audio_metadata_extracts_word_boundaries() {
        let mut events = Vec::new();
        let msg = "Path:audio.metadata\r\n\r\n\
            {\"Metadata\":[{\"Type\":\"WordBoundary\",\"Data\":\
            {\"Offset\":500,\"Duration\":200,\"text\":{\"Text\":\"hi\"}}}]}";
        let stop = EdgeTts::handle_text_message(msg, &mut events);
        assert!(!stop);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn handle_unknown_path_continues() {
        let mut events = Vec::new();
        let msg = "Path:turn.start\r\n\r\n{}";
        let stop = EdgeTts::handle_text_message(msg, &mut events);
        assert!(!stop);
        assert!(events.is_empty());
    }

    #[test]
    fn handle_missing_path_header_continues() {
        let mut events = Vec::new();
        let msg = "Content-Type:text\r\n\r\nbody";
        let stop = EdgeTts::handle_text_message(msg, &mut events);
        assert!(!stop);
        assert!(events.is_empty());
    }
}
